//! Backend for DRM/KMS for raw rendering directly to the screen.
//!
//! This strategy uses dumb buffers for rendering.

use drm::buffer::{Buffer, DrmFourcc};
use drm::control::dumbbuffer::{DumbBuffer, DumbMapping};
use drm::control::{connector, crtc, framebuffer, plane, Device as CtrlDevice};
use drm::Device;

use raw_window_handle::{DrmDisplayHandle, DrmWindowHandle};

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

    /// The connector to use.
    connector: connector::Handle,

    /// The CRTC to render to.
    crtc: crtc::Info,

    /// The dumb buffer we're using as a buffer.
    buffer: Option<BufferSet>,
}

/// The buffer implementation.
pub(crate) struct BufferImpl<'a> {
    /// The mapping of the dump buffer.
    mapping: DumbMapping<'a>,

    /// The framebuffer.
    fb: framebuffer::Handle,

    /// The current size.
    size: (NonZeroU32, NonZeroU32),

    /// The display implementation.
    display: &'a KmsDisplayImpl,

    /// The zero buffer.
    zeroes: &'a [u32],
}

/// The combined frame buffer and dumb buffer.
#[derive(Debug)]
struct BufferSet {
    /// The frame buffer.
    fb: framebuffer::Handle,

    /// The dumb buffer.
    db: DumbBuffer,

    /// Equivalent mapping for reading.
    zeroes: Box<[u32]>,
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
        let crtc = match plane_info.crtc() {
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

        // Use a preferred connector or just select the first one.
        let connector = handles
            .connectors
            .iter()
            .flat_map(|handle| display.get_connector(*handle, false))
            .find_map(|conn| {
                if conn.state() == connector::State::Connected {
                    Some(conn.handle())
                } else {
                    None
                }
            })
            .or_else(|| handles.connectors.first().copied())
            .swbuf_err("failed to find a valid connector")?;

        Ok(Self {
            crtc: display
                .get_crtc(crtc)
                .swbuf_err("failed to get CRTC info")?,
            connector,
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

        // Create a new buffer.
        let buffer = BufferSet::new(&self.display, width, height)?;

        // Set the framebuffer in the CRTC info.
        // TODO: This requires root access, find a way that doesn't!
        self.display
            .set_crtc(
                self.crtc.handle(),
                Some(buffer.fb),
                self.crtc.position(),
                &[self.connector],
                self.crtc.mode(),
            )
            .swbuf_err("failed to set CRTC")?;

        self.buffer = Some(buffer);

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
        let mapping = self
            .display
            .map_dumb_buffer(&mut set.db)
            .swbuf_err("failed to map dumb buffer")?;

        Ok(BufferImpl {
            mapping,
            size,
            fb: set.fb,
            display: &self.display,
            zeroes: &set.zeroes,
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
                &[self.connector],
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
        todo!()
    }

    #[inline]
    pub fn present_with_damage(self, damage: &[crate::Rect]) -> Result<(), SoftBufferError> {
        let rectangles = damage
            .iter()
            .map(|&rect| {
                let err = || SoftBufferError::DamageOutOfRange { rect };
                Ok(drm_sys::drm_clip_rect {
                    x1: rect.x.try_into().map_err(|_| err())?,
                    y1: rect.y.try_into().map_err(|_| err())?,
                    x2: rect
                        .x
                        .checked_add(rect.width.get())
                        .and_then(|x| x.try_into().ok())
                        .ok_or_else(err)?,
                    y2: rect
                        .y
                        .checked_add(rect.height.get())
                        .and_then(|y| y.try_into().ok())
                        .ok_or_else(err)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.display
            .dirty_framebuffer(self.fb, &rectangles)
            .swbuf_err("failed to dirty framebuffer")?;

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

impl BufferSet {
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

        Ok(BufferSet {
            fb,
            db,
            zeroes: vec![0; width.get() as usize * height.get() as usize].into_boxed_slice(),
        })
    }

    /// Get the size of this buffer.
    pub(crate) fn size(&self) -> (NonZeroU32, NonZeroU32) {
        let (width, height) = self.db.size();

        NonZeroU32::new(width)
            .and_then(|width| NonZeroU32::new(height).map(|height| (width, height)))
            .expect("buffer size is zero")
    }
}
