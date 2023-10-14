//! Backend for DRM/KMS for raw rendering directly to the screen.
//!
//! This strategy uses dumb buffers for rendering.

use drm::buffer::{Buffer, DrmFourcc};
use drm::control::dumbbuffer::{DumbBuffer, DumbMapping};
use drm::control::{
    connector, crtc, framebuffer, plane, ClipRect, Device as CtrlDevice, PageFlipFlags,
};
use drm::Device;

use raw_window_handle::{DrmDisplayHandle, DrmWindowHandle};

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::os::unix::io::{AsFd, BorrowedFd};
use std::rc::Rc;

use crate::error::{SoftBufferError, SwResultExt};

#[derive(Debug)]
pub(crate) struct KmsDisplayImpl {
    /// The underlying raw device file descriptor.
    ///
    /// Once rwh v0.6 support is merged, this an be made safe. Until then,
    /// we use this hacky workaround, since this FD's validity is guaranteed by
    /// the unsafe constructor.
    fd: BorrowedFd<'static>,
}

impl AsFd for KmsDisplayImpl {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd
    }
}

impl Device for KmsDisplayImpl {}
impl CtrlDevice for KmsDisplayImpl {}

impl KmsDisplayImpl {
    /// SAFETY: The underlying fd must not outlive the display.
    pub(crate) unsafe fn new(handle: DrmDisplayHandle) -> Result<KmsDisplayImpl, SoftBufferError> {
        let fd = handle.fd;
        if fd == -1 {
            return Err(SoftBufferError::IncompleteDisplayHandle);
        }

        // SAFETY: Invariants guaranteed by the user.
        let fd = unsafe { BorrowedFd::borrow_raw(fd) };

        Ok(KmsDisplayImpl { fd })
    }
}

/// All the necessary types for the Drm/Kms backend.
#[derive(Debug)]
pub(crate) struct KmsImpl {
    /// The display implementation.
    display: Rc<KmsDisplayImpl>,

    /// The connectors to use.
    connectors: Vec<connector::Handle>,

    /// The CRTC to render to.
    crtc: crtc::Info,

    /// The dumb buffer we're using as a buffer.
    buffer: Option<Buffers>,
}

#[derive(Debug)]
struct Buffers {
    /// The involved set of buffers.
    buffers: [SharedBuffer; 2],

    /// Whether to use the first buffer or the second buffer as the front buffer.
    first_is_front: bool,

    /// A buffer full of zeroes.
    zeroes: Box<[u32]>,
}

/// The buffer implementation.
pub(crate) struct BufferImpl<'a> {
    /// The mapping of the dump buffer.
    mapping: DumbMapping<'a>,

    /// The framebuffer object of the current front buffer.
    front_fb: framebuffer::Handle,

    /// The CRTC handle.
    crtc_handle: crtc::Handle,

    /// This is used to change the front buffer.
    first_is_front: &'a mut bool,

    /// Buffer full of zeroes.
    zeroes: &'a [u32],

    /// The current size.
    size: (NonZeroU32, NonZeroU32),

    /// The display implementation.
    display: &'a KmsDisplayImpl,

    /// Age of the front buffer.
    front_age: &'a mut u8,

    /// Age of the back buffer.
    back_age: &'a mut u8,
}

/// The combined frame buffer and dumb buffer.
#[derive(Debug)]
struct SharedBuffer {
    /// The frame buffer.
    fb: framebuffer::Handle,

    /// The dumb buffer.
    db: DumbBuffer,

    /// The age of this buffer.
    age: u8,
}

impl KmsImpl {
    /// Create a new KMS backend.
    ///
    /// # Safety
    ///
    /// The plane must be valid for the lifetime of the backend.
    pub(crate) unsafe fn new(
        window_handle: DrmWindowHandle,
        display: Rc<KmsDisplayImpl>,
    ) -> Result<Self, SoftBufferError> {
        log::trace!("new: window_handle={:X}", window_handle.plane);

        // Make sure that the window handle is valid.
        let plane_handle = match NonZeroU32::new(window_handle.plane) {
            Some(handle) => plane::Handle::from(handle),
            None => return Err(SoftBufferError::IncompleteWindowHandle),
        };

        let plane_info = display
            .get_plane(plane_handle)
            .swbuf_err("failed to get plane info")?;
        let handles = display
            .resource_handles()
            .swbuf_err("failed to get resource handles")?;

        // Use either the attached CRTC or the primary CRTC.
        let crtc = {
            let handle = match plane_info.crtc() {
                Some(crtc) => crtc,
                None => {
                    log::warn!("no CRTC attached to plane, falling back to primary CRTC");
                    handles
                        .filter_crtcs(plane_info.possible_crtcs())
                        .first()
                        .copied()
                        .swbuf_err("failed to find a primary CRTC")?
                }
            };

            // Get info about the CRTC.
            display
                .get_crtc(handle)
                .swbuf_err("failed to get CRTC info")?
        };

        // Figure out all of the encoders that are attached to this CRTC.
        let encoders = handles
            .encoders
            .iter()
            .flat_map(|handle| display.get_encoder(*handle))
            .filter(|encoder| encoder.crtc() == Some(crtc.handle()))
            .map(|encoder| encoder.handle())
            .collect::<HashSet<_>>();

        // Get a list of every connector that the CRTC is connected to via encoders.
        let connectors = handles
            .connectors
            .iter()
            .flat_map(|handle| display.get_connector(*handle, false))
            .filter(|connector| {
                connector
                    .current_encoder()
                    .map_or(false, |encoder| encoders.contains(&encoder))
            })
            .map(|info| info.handle())
            .collect::<Vec<_>>();

        Ok(Self {
            crtc,
            connectors,
            display,
            buffer: None,
        })
    }

    /// Resize the internal buffer to the given size.
    pub(crate) fn resize(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), SoftBufferError> {
        // Don't resize if we don't have to.
        if let Some(buffer) = &self.buffer {
            let (buffer_width, buffer_height) = buffer.size();
            if buffer_width == width && buffer_height == height {
                return Ok(());
            }
        }

        // Create a new buffer set.
        let front_buffer = SharedBuffer::new(&self.display, width, height)?;
        let back_buffer = SharedBuffer::new(&self.display, width, height)?;

        self.buffer = Some(Buffers {
            first_is_front: true,
            buffers: [front_buffer, back_buffer],
            zeroes: vec![0; width.get() as usize * height.get() as usize].into_boxed_slice(),
        });

        Ok(())
    }

    /// Fetch the buffer from the window.
    pub(crate) fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        // TODO: Implement this!
        Err(SoftBufferError::Unimplemented)
    }

    /// Get a mutable reference to the buffer.
    pub(crate) fn buffer_mut(&mut self) -> Result<BufferImpl<'_>, SoftBufferError> {
        // Map the dumb buffer.
        let set = self
            .buffer
            .as_mut()
            .expect("Must set size of surface before calling `buffer_mut()`");

        let size = set.size();

        let [first_buffer, second_buffer] = &mut set.buffers;
        let (front_buffer, back_buffer) = if set.first_is_front {
            (first_buffer, second_buffer)
        } else {
            (second_buffer, first_buffer)
        };

        let front_fb = front_buffer.fb;
        let front_age = &mut front_buffer.age;
        let back_age = &mut back_buffer.age;

        let mapping = self
            .display
            .map_dumb_buffer(&mut front_buffer.db)
            .swbuf_err("failed to map dumb buffer")?;

        Ok(BufferImpl {
            mapping,
            size,
            first_is_front: &mut set.first_is_front,
            front_fb,
            crtc_handle: self.crtc.handle(),
            display: &self.display,
            zeroes: &set.zeroes,
            front_age,
            back_age,
        })
    }
}

impl Drop for KmsImpl {
    fn drop(&mut self) {
        // Map the CRTC to the information that was there before.
        self.display
            .set_crtc(
                self.crtc.handle(),
                self.crtc.framebuffer(),
                self.crtc.position(),
                &self.connectors,
                self.crtc.mode(),
            )
            .ok();
    }
}

impl BufferImpl<'_> {
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        // drm-rs doesn't let us have the immutable reference... so just use a bunch of zeroes.
        // TODO: There has to be a better way of doing this!
        self.zeroes
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        bytemuck::cast_slice_mut(self.mapping.as_mut())
    }

    #[inline]
    pub fn age(&self) -> u8 {
        *self.front_age
    }

    #[inline]
    pub fn present_with_damage(self, damage: &[crate::Rect]) -> Result<(), SoftBufferError> {
        let rectangles = damage
            .iter()
            .map(|&rect| {
                let err = || SoftBufferError::DamageOutOfRange { rect };
                Ok(ClipRect::new(
                    rect.x.try_into().map_err(|_| err())?,
                    rect.y.try_into().map_err(|_| err())?,
                    rect.x
                        .checked_add(rect.width.get())
                        .and_then(|x| x.try_into().ok())
                        .ok_or_else(err)?,
                    rect.y
                        .checked_add(rect.height.get())
                        .and_then(|y| y.try_into().ok())
                        .ok_or_else(err)?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Dirty the framebuffer with out damage rectangles.
        //
        // Some drivers don't support this, so we just ignore the `ENOSYS` error.
        // TODO: It would be nice to not have to heap-allocate the above rectangles if we know that
        // this is going to fail. Low hanging fruit PR: add a flag that's set to false if this
        // returns `ENOSYS` and check that before allocating the above and running this.
        match self.display.dirty_framebuffer(self.front_fb, &rectangles) {
            Ok(()) => {}
            Err(drm::SystemError::Unknown { errno })
                if errno as i32 == rustix::io::Errno::NOSYS.raw_os_error() => {}
            Err(e) => {
                return Err(SoftBufferError::PlatformError(
                    Some("failed to dirty framebuffer".into()),
                    Some(e.into()),
                ));
            }
        }

        // Swap the buffers.
        // TODO: Use atomic commits here!
        self.display
            .page_flip(self.crtc_handle, self.front_fb, PageFlipFlags::EVENT, None)
            .swbuf_err("failed to page flip")?;

        // Flip the front and back buffers.
        *self.first_is_front = !*self.first_is_front;

        // Set the ages.
        *self.front_age = 1;
        if *self.back_age != 0 {
            *self.back_age += 1;
        }

        Ok(())
    }

    #[inline]
    pub fn present(self) -> Result<(), SoftBufferError> {
        let (width, height) = self.size;
        self.present_with_damage(&[crate::Rect {
            x: 0,
            y: 0,
            width,
            height,
        }])
    }
}

impl SharedBuffer {
    /// Create a new buffer set.
    pub(crate) fn new(
        display: &KmsDisplayImpl,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<Self, SoftBufferError> {
        let db = display
            .create_dumb_buffer((width.get(), height.get()), DrmFourcc::Xrgb8888, 32)
            .swbuf_err("failed to create dumb buffer")?;
        let fb = display
            .add_framebuffer(&db, 24, 32)
            .swbuf_err("failed to add framebuffer")?;

        Ok(SharedBuffer { fb, db, age: 0 })
    }

    /// Get the size of this buffer.
    pub(crate) fn size(&self) -> (NonZeroU32, NonZeroU32) {
        let (width, height) = self.db.size();

        NonZeroU32::new(width)
            .and_then(|width| NonZeroU32::new(height).map(|height| (width, height)))
            .expect("buffer size is zero")
    }
}

impl Buffers {
    /// Get the size of this buffer.
    pub(crate) fn size(&self) -> (NonZeroU32, NonZeroU32) {
        self.buffers[0].size()
    }
}
