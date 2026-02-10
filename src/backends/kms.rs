//! Backend for DRM/KMS for raw rendering directly to the screen.
//!
//! This strategy uses dumb buffers for rendering.

use drm::buffer::{Buffer, DrmFourcc};
use drm::control::dumbbuffer::{DumbBuffer, DumbMapping};
use drm::control::{
    connector, crtc, framebuffer, plane, ClipRect, Device as CtrlDevice, PageFlipFlags,
};
use drm::Device;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

use std::collections::HashSet;
use std::fmt;
use std::mem::size_of;
use std::num::NonZeroU32;
use std::os::unix::io::{AsFd, BorrowedFd};
use std::slice;
use std::sync::Arc;

use crate::backend_interface::*;
use crate::error::{InitError, SoftBufferError, SwResultExt};
use crate::{util, Pixel};

#[derive(Debug, Clone)]
struct DrmDevice<'surface> {
    /// The underlying raw display file descriptor.
    fd: BorrowedFd<'surface>,
}

impl AsFd for DrmDevice<'_> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd
    }
}

impl Device for DrmDevice<'_> {}
impl CtrlDevice for DrmDevice<'_> {}

#[derive(Debug)]
pub(crate) struct KmsDisplayImpl<D: ?Sized> {
    device: DrmDevice<'static>,

    /// Holds a reference to the display.
    _display: D,
}

impl<D: HasDisplayHandle + ?Sized> ContextInterface<D> for Arc<KmsDisplayImpl<D>> {
    fn new(display: D) -> Result<Self, InitError<D>>
    where
        D: Sized,
    {
        let RawDisplayHandle::Drm(drm) = display.display_handle()?.as_raw() else {
            return Err(InitError::Unsupported(display));
        };
        if drm.fd == -1 {
            return Err(SoftBufferError::IncompleteDisplayHandle.into());
        }

        // SAFETY: Invariants guaranteed by the user.
        let fd = unsafe { BorrowedFd::borrow_raw(drm.fd) };

        Ok(Arc::new(KmsDisplayImpl {
            device: DrmDevice { fd },
            _display: display,
        }))
    }
}

/// All the necessary types for the Drm/Kms backend.
#[derive(Debug)]
pub(crate) struct KmsImpl<D: ?Sized, W: ?Sized> {
    /// The display implementation.
    display: Arc<KmsDisplayImpl<D>>,

    /// The connectors to use.
    connectors: Vec<connector::Handle>,

    /// The CRTC to render to.
    crtc: crtc::Info,

    /// The dumb buffer we're using as a buffer.
    buffer: Option<Buffers>,

    /// Window handle that we are keeping around.
    window_handle: W,
}

#[derive(Debug)]
struct Buffers {
    /// The involved set of buffers.
    buffers: [SharedBuffer; 2],

    /// Whether to use the first buffer or the second buffer as the front buffer.
    first_is_front: bool,
}

/// The buffer implementation.
pub(crate) struct BufferImpl<'surface> {
    /// The mapping of the dump buffer.
    mapping: DumbMapping<'surface>,

    /// The framebuffer object of the current front buffer.
    front_fb: framebuffer::Handle,

    /// The CRTC handle.
    crtc_handle: crtc::Handle,

    /// This is used to change the front buffer.
    first_is_front: &'surface mut bool,

    /// The current size.
    size: (NonZeroU32, NonZeroU32),

    /// The device file descriptor.
    device: DrmDevice<'surface>,

    /// Age of the front buffer.
    front_age: &'surface mut u8,

    /// Age of the back buffer.
    back_age: &'surface mut u8,
}

impl fmt::Debug for BufferImpl<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // FIXME: Derive instead once `DumbMapping` impls `Debug`.
        f.debug_struct("BufferImpl").finish_non_exhaustive()
    }
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

impl<D: HasDisplayHandle + ?Sized, W: HasWindowHandle> SurfaceInterface<D, W> for KmsImpl<D, W> {
    type Context = Arc<KmsDisplayImpl<D>>;
    type Buffer<'surface>
        = BufferImpl<'surface>
    where
        Self: 'surface;

    /// Create a new KMS backend.
    fn new(window: W, display: &Arc<KmsDisplayImpl<D>>) -> Result<Self, InitError<W>> {
        let device = &display.device;

        // Make sure that the window handle is valid.
        let RawWindowHandle::Drm(drm) = window.window_handle()?.as_raw() else {
            return Err(InitError::Unsupported(window));
        };
        let plane_handle =
            NonZeroU32::new(drm.plane).ok_or(SoftBufferError::IncompleteWindowHandle)?;
        let plane_handle = plane::Handle::from(plane_handle);

        let plane_info = device
            .get_plane(plane_handle)
            .swbuf_err("failed to get plane info")?;
        let handles = device
            .resource_handles()
            .swbuf_err("failed to get resource handles")?;

        // Use either the attached CRTC or the primary CRTC.
        let crtc = {
            let handle = match plane_info.crtc() {
                Some(crtc) => crtc,
                None => {
                    tracing::warn!("no CRTC attached to plane, falling back to primary CRTC");
                    handles
                        .filter_crtcs(plane_info.possible_crtcs())
                        .first()
                        .copied()
                        .swbuf_err("failed to find a primary CRTC")?
                }
            };

            // Get info about the CRTC.
            device
                .get_crtc(handle)
                .swbuf_err("failed to get CRTC info")?
        };

        // Figure out all of the encoders that are attached to this CRTC.
        let encoders = handles
            .encoders
            .iter()
            .flat_map(|handle| device.get_encoder(*handle))
            .filter(|encoder| encoder.crtc() == Some(crtc.handle()))
            .map(|encoder| encoder.handle())
            .collect::<HashSet<_>>();

        // Get a list of every connector that the CRTC is connected to via encoders.
        let connectors = handles
            .connectors
            .iter()
            .flat_map(|handle| device.get_connector(*handle, false))
            .filter(|connector| {
                connector
                    .current_encoder()
                    .is_some_and(|encoder| encoders.contains(&encoder))
            })
            .map(|info| info.handle())
            .collect::<Vec<_>>();

        Ok(Self {
            crtc,
            connectors,
            display: display.clone(),
            buffer: None,
            window_handle: window,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
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
        });

        Ok(())
    }

    /*
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        // TODO: Implement this!
    }
    */

    fn next_buffer(&mut self) -> Result<BufferImpl<'_>, SoftBufferError> {
        // Map the dumb buffer.
        let set = self
            .buffer
            .as_mut()
            .expect("Must set size of surface before calling `next_buffer()`");

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
            .device
            .map_dumb_buffer(&mut front_buffer.db)
            .swbuf_err("failed to map dumb buffer")?;

        Ok(BufferImpl {
            mapping,
            size,
            first_is_front: &mut set.first_is_front,
            front_fb,
            crtc_handle: self.crtc.handle(),
            device: self.display.device.clone(),
            front_age,
            back_age,
        })
    }
}

impl<D: ?Sized, W: ?Sized> Drop for KmsImpl<D, W> {
    fn drop(&mut self) {
        // Map the CRTC to the information that was there before.
        self.display
            .device
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

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width().get() * 4).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        self.size.0
    }

    fn height(&self) -> NonZeroU32 {
        self.size.1
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        let ptr = self.mapping.as_mut_ptr().cast::<Pixel>();
        let len = self.mapping.len() / size_of::<Pixel>();
        debug_assert_eq!(self.mapping.len() % size_of::<Pixel>(), 0);
        // SAFETY: `&mut [u8]` can be reinterpreted as `&mut [Pixel]`, assuming that the allocation
        // is aligned to at least a multiple of 4 bytes.
        unsafe { slice::from_raw_parts_mut(ptr, len) }
    }

    #[inline]
    fn age(&self) -> u8 {
        *self.front_age
    }

    #[inline]
    fn present_with_damage(self, damage: &[crate::Rect]) -> Result<(), SoftBufferError> {
        let rectangles: Vec<_> = damage
            .iter()
            .map(|rect| {
                ClipRect::new(
                    util::to_u16_saturating(rect.x),
                    util::to_u16_saturating(rect.y),
                    util::to_u16_saturating(rect.x.saturating_add(rect.width.get())),
                    util::to_u16_saturating(rect.y.saturating_add(rect.height.get())),
                )
            })
            .collect();

        // Dirty the framebuffer with out damage rectangles.
        //
        // Some drivers don't support this, so we just ignore the `ENOSYS` error.
        // TODO: It would be nice to not have to heap-allocate the above rectangles if we know that
        // this is going to fail. Low hanging fruit PR: add a flag that's set to false if this
        // returns `ENOSYS` and check that before allocating the above and running this.
        match self.device.dirty_framebuffer(self.front_fb, &rectangles) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(rustix::io::Errno::NOSYS.raw_os_error()) => {}
            Err(e) => {
                return Err(SoftBufferError::PlatformError(
                    Some("failed to dirty framebuffer".into()),
                    Some(e.into()),
                ));
            }
        }

        // Swap the buffers.
        // TODO: Use atomic commits here!
        self.device
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
}

impl SharedBuffer {
    /// Create a new buffer set.
    pub(crate) fn new<D: ?Sized>(
        display: &KmsDisplayImpl<D>,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<Self, SoftBufferError> {
        let db = display
            .device
            .create_dumb_buffer((width.get(), height.get()), DrmFourcc::Xrgb8888, 32)
            .swbuf_err("failed to create dumb buffer")?;
        let fb = display
            .device
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
