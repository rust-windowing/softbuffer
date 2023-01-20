//! Implementation of software buffering for X11.
//!
//! This module converts the input buffer into an XImage and then sends it over the wire to be
//! drawn by the X server. The SHM extension is used if available.

#![allow(clippy::uninlined_format_args)]

use crate::SoftBufferError;
use nix::libc::{shmat, shmctl, shmdt, shmget, IPC_PRIVATE, IPC_RMID};
use raw_window_handle::{XcbDisplayHandle, XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle};
use std::ptr::{null_mut, NonNull};
use std::{fmt, io, mem, rc::Rc};

use x11_dl::xlib::Display;
use x11_dl::xlib_xcb::Xlib_xcb;

use x11rb::connection::{Connection, SequenceNumber};
use x11rb::cookie::Cookie;
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::xcb_ffi::XCBConnection;

pub struct X11DisplayImpl {
    /// The handle to the XCB connection.
    connection: XCBConnection,

    /// SHM extension is available.
    is_shm_available: bool,
}

impl X11DisplayImpl {
    pub(crate) unsafe fn from_xlib(
        display_handle: XlibDisplayHandle,
    ) -> Result<X11DisplayImpl, SoftBufferError> {
        // TODO: We should cache the shared libraries.

        // Try to open the XlibXCB shared library.
        let lib_xcb = Xlib_xcb::open().swbuf_err("Failed to open XlibXCB shared library")?;

        // Validate the display handle to ensure we can use it.
        if display_handle.display.is_null() {
            return Err(SoftBufferError::IncompleteDisplayHandle);
        }

        // Get the underlying XCB connection.
        // SAFETY: The user has asserted that the display handle is valid.
        let connection =
            unsafe { (lib_xcb.XGetXCBConnection)(display_handle.display as *mut Display) };

        // Construct the equivalent XCB display and window handles.
        let mut xcb_display_handle = XcbDisplayHandle::empty();
        xcb_display_handle.connection = connection;
        xcb_display_handle.screen = display_handle.screen;

        // SAFETY: If the user passed in valid Xlib handles, then these are valid XCB handles.
        unsafe { Self::from_xcb(xcb_display_handle) }
    }

    /// Create a new `X11Impl` from a `XcbWindowHandle` and `XcbDisplayHandle`.
    ///
    /// # Safety
    ///
    /// The `XcbWindowHandle` and `XcbDisplayHandle` must be valid.
    pub(crate) unsafe fn from_xcb(
        display_handle: XcbDisplayHandle,
    ) -> Result<Self, SoftBufferError> {
        // Check that the handle is valid.
        if display_handle.connection.is_null() {
            return Err(SoftBufferError::IncompleteDisplayHandle);
        }

        // Wrap the display handle in an x11rb connection.
        // SAFETY: We don't own the connection, so don't drop it. We also assert that the connection is valid.
        let connection = {
            let result =
                unsafe { XCBConnection::from_raw_xcb_connection(display_handle.connection, false) };

            result.swbuf_err("Failed to wrap XCB connection")?
        };

        let is_shm_available = is_shm_available(&connection);
        if !is_shm_available {
            log::warn!("SHM extension is not available. Performance may be poor.");
        }

        Ok(Self {
            connection,
            is_shm_available,
        })
    }
}

/// The handle to an X11 drawing context.
pub struct X11Impl {
    /// X display this window belongs to.
    display: Rc<X11DisplayImpl>,

    /// The window to draw to.
    window: xproto::Window,

    /// The graphics context to use when drawing.
    gc: xproto::Gcontext,

    /// The depth (bits per pixel) of the drawing context.
    depth: u8,

    /// The buffer we draw to.
    buffer: Buffer,

    /// The current buffer width.
    width: u16,

    /// The current buffer height.
    height: u16,
}

/// The buffer that is being drawn to.
enum Buffer {
    /// A buffer implemented using shared memory to prevent unnecessary copying.
    Shm(ShmBuffer),

    /// A normal buffer that we send over the wire.
    Wire(Vec<u32>),
}

struct ShmBuffer {
    /// The shared memory segment, paired with its ID.
    seg: Option<(ShmSegment, shm::Seg)>,

    /// A cookie indicating that the shared memory segment is ready to be used.
    ///
    /// We can't soundly read from or write to the SHM segment until the X server is done processing the
    /// `shm::PutImage` request. However, the X server handles requests in order, which means that, if
    /// we send a very small request after the `shm::PutImage` request, then the X server will have to
    /// process that request before it can process the `shm::PutImage` request. Therefore, we can use
    /// the reply to that small request to determine when the `shm::PutImage` request is done.
    ///
    /// In this case, we use `GetInputFocus` since it is a very small request.
    ///
    /// We store the sequence number instead of the `Cookie` since we cannot hold a self-referential
    /// reference to the `connection` field.
    done_processing: Option<SequenceNumber>,
}

impl X11Impl {
    /// Create a new `X11Impl` from a `XlibWindowHandle` and `XlibDisplayHandle`.
    ///
    /// # Safety
    ///
    /// The `XlibWindowHandle` and `XlibDisplayHandle` must be valid.
    pub unsafe fn from_xlib(
        window_handle: XlibWindowHandle,
        display: Rc<X11DisplayImpl>,
    ) -> Result<Self, SoftBufferError> {
        let mut xcb_window_handle = XcbWindowHandle::empty();
        xcb_window_handle.window = window_handle.window as _;
        xcb_window_handle.visual_id = window_handle.visual_id as _;

        // SAFETY: If the user passed in valid Xlib handles, then these are valid XCB handles.
        unsafe { Self::from_xcb(xcb_window_handle, display) }
    }

    /// Create a new `X11Impl` from a `XcbWindowHandle` and `XcbDisplayHandle`.
    ///
    /// # Safety
    ///
    /// The `XcbWindowHandle` and `XcbDisplayHandle` must be valid.
    pub(crate) unsafe fn from_xcb(
        window_handle: XcbWindowHandle,
        display: Rc<X11DisplayImpl>,
    ) -> Result<Self, SoftBufferError> {
        log::trace!("new: window_handle={:X}", window_handle.window,);

        // Check that the handle is valid.
        if window_handle.window == 0 {
            return Err(SoftBufferError::IncompleteWindowHandle);
        }

        let window = window_handle.window;

        // Run in parallel: start getting the window depth.
        let geometry_token = display
            .connection
            .get_geometry(window)
            .swbuf_err("Failed to send geometry request")?;

        // Create a new graphics context to draw to.
        let gc = display
            .connection
            .generate_id()
            .swbuf_err("Failed to generate GC ID")?;
        display
            .connection
            .create_gc(
                gc,
                window,
                &xproto::CreateGCAux::new().graphics_exposures(0),
            )
            .swbuf_err("Failed to send GC creation request")?
            .check()
            .swbuf_err("Failed to create GC")?;

        // Finish getting the depth of the window.
        let geometry_reply = geometry_token
            .reply()
            .swbuf_err("Failed to get geometry reply")?;

        // See if SHM is available.
        let buffer = if display.is_shm_available {
            // SHM is available.
            Buffer::Shm(ShmBuffer {
                seg: None,
                done_processing: None,
            })
        } else {
            // SHM is not available.
            Buffer::Wire(Vec::new())
        };

        Ok(Self {
            display,
            window,
            gc,
            depth: geometry_reply.depth,
            buffer,
            width: 0,
            height: 0,
        })
    }

    /// Resize the internal buffer to the given width and height.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        log::trace!(
            "resize: window={:X}, size={}x{}",
            self.window,
            width,
            height
        );

        // Width and height should fit in u16.
        let width: u16 = width.try_into().expect("Width too large");
        let height: u16 = height.try_into().expect("Height too large");

        if width == self.width && height == self.height {
            // Nothing to do.
            return;
        }

        match self.buffer.resize(&self.display.connection, width, height) {
            Ok(()) => {
                // We successfully resized the buffer.
                self.width = width;
                self.height = height;
            }

            Err(e) => {
                log::error!("Failed to resize window: {}", e);
            }
        }
    }

    /// Get a mutable reference to the buffer.
    pub(crate) fn buffer_mut(&mut self) -> &mut [u32] {
        log::trace!("buffer_mut: window={:X}", self.window);

        let buffer = self
            .buffer
            .buffer_mut(&self.display.connection)
            .expect("Failed to get buffer");

        // Crop it down to the window size.
        &mut buffer[..total_len(self.width, self.height) / 4]
    }

    /// Push the buffer to the window.
    pub(crate) fn present(&mut self) -> Result<(), SoftBufferError> {
        log::trace!("present: window={:X}", self.window);

        let result = match self.buffer {
            Buffer::Wire(ref wire) => {
                // This is a suboptimal strategy, raise a stink in the debug logs.
                log::debug!("Falling back to non-SHM method for window drawing.");

                self.display
                    .connection
                    .put_image(
                        xproto::ImageFormat::Z_PIXMAP,
                        self.window,
                        self.gc,
                        self.width,
                        self.height,
                        0,
                        0,
                        0,
                        self.depth,
                        bytemuck::cast_slice(wire),
                    )
                    .map(|c| c.ignore_error())
                    .push_err()
            }

            Buffer::Shm(ref mut shm) => {
                // If the X server is still processing the last image, wait for it to finish.
                shm.finish_wait(&self.display.connection)
                    .and_then(|()| {
                        // Put the image into the window.
                        if let Some((_, segment_id)) = shm.seg {
                            self.display
                                .connection
                                .shm_put_image(
                                    self.window,
                                    self.gc,
                                    self.width,
                                    self.height,
                                    0,
                                    0,
                                    self.width,
                                    self.height,
                                    0,
                                    0,
                                    self.depth,
                                    xproto::ImageFormat::Z_PIXMAP.into(),
                                    false,
                                    segment_id,
                                    0,
                                )
                                .push_err()
                                .map(|c| c.ignore_error())
                        } else {
                            Ok(())
                        }
                    })
                    .and_then(|()| {
                        // Send a short request to act as a notification for when the X server is done processing the image.
                        shm.begin_wait(&self.display.connection)
                    })
            }
        };

        result.map_err(|err| {
            SoftBufferError::PlatformError(
                Some("Failed to draw image to window".to_string()),
                Some(Box::new(err)),
            )
        })
    }
}

impl Buffer {
    /// Resize the buffer to the given size.
    fn resize(
        &mut self,
        conn: &impl Connection,
        width: u16,
        height: u16,
    ) -> Result<(), PushBufferError> {
        match self {
            Buffer::Shm(ref mut shm) => shm.alloc_segment(conn, total_len(width, height)),
            Buffer::Wire(wire) => {
                wire.resize(total_len(width, height), 0);
                Ok(())
            }
        }
    }

    /// Get a mutable reference to the buffer.
    fn buffer_mut(&mut self, conn: &impl Connection) -> Result<&mut [u32], PushBufferError> {
        match self {
            Buffer::Shm(ref mut shm) => shm.as_mut(conn),
            Buffer::Wire(wire) => Ok(wire),
        }
    }
}

impl ShmBuffer {
    /// Allocate a new `ShmSegment` of the given size.
    fn alloc_segment(
        &mut self,
        conn: &impl Connection,
        size: usize,
    ) -> Result<(), PushBufferError> {
        // Round the size up to the next power of two to prevent frequent reallocations.
        let size = size.next_power_of_two();

        // Get the size of the segment currently in use.
        let needs_realloc = match self.seg {
            Some((ref seg, _)) => seg.size() < size,
            None => true,
        };

        // Reallocate if necessary.
        if needs_realloc {
            let new_seg = ShmSegment::new(size)?;
            self.associate(conn, new_seg)?;
        }

        Ok(())
    }

    /// Get the SHM buffer as a mutable reference.
    fn as_mut(&mut self, conn: &impl Connection) -> Result<&mut [u32], PushBufferError> {
        // Make sure that, if we're waiting for the X server to finish processing the last image,
        // that we finish the wait.
        self.finish_wait(conn)?;

        match self.seg.as_mut() {
            Some((seg, _)) => {
                // SAFETY: No other code should be able to access the segment.
                Ok(bytemuck::cast_slice_mut(unsafe { seg.as_mut() }))
            }
            None => {
                // Nothing has been allocated yet.
                Ok(&mut [])
            }
        }
    }

    /// Associate an SHM segment with the server.
    fn associate(
        &mut self,
        conn: &impl Connection,
        seg: ShmSegment,
    ) -> Result<(), PushBufferError> {
        // Register the guard.
        let new_id = conn.generate_id()?;
        conn.shm_attach(new_id, seg.id(), true)?.ignore_error();

        // Take out the old one and detach it.
        if let Some((old_seg, old_id)) = self.seg.replace((seg, new_id)) {
            // Wait for the old segment to finish processing.
            self.finish_wait(conn)?;

            conn.shm_detach(old_id)?.ignore_error();

            // Drop the old segment.
            drop(old_seg);
        }

        Ok(())
    }

    /// Begin waiting for the SHM processing to finish.
    fn begin_wait(&mut self, c: &impl Connection) -> Result<(), PushBufferError> {
        let cookie = c.get_input_focus()?.sequence_number();
        let old_cookie = self.done_processing.replace(cookie);
        debug_assert!(old_cookie.is_none());
        Ok(())
    }

    /// Wait for the SHM processing to finish.
    fn finish_wait(&mut self, c: &impl Connection) -> Result<(), PushBufferError> {
        if let Some(done_processing) = self.done_processing.take() {
            // Cast to a cookie and wait on it.
            let cookie = Cookie::<_, xproto::GetInputFocusReply>::new(c, done_processing);
            cookie.reply()?;
        }

        Ok(())
    }
}

struct ShmSegment {
    id: i32,
    ptr: NonNull<i8>,
    size: usize,
}

impl ShmSegment {
    /// Create a new `ShmSegment` with the given size.
    fn new(size: usize) -> io::Result<Self> {
        unsafe {
            // Create the shared memory segment.
            let id = shmget(IPC_PRIVATE, size, 0o600);
            if id == -1 {
                return Err(io::Error::last_os_error());
            }

            // Map the SHM to our memory space.
            let ptr = {
                let ptr = shmat(id, null_mut(), 0);
                match NonNull::new(ptr as *mut i8) {
                    Some(ptr) => ptr,
                    None => {
                        shmctl(id, IPC_RMID, null_mut());
                        return Err(io::Error::last_os_error());
                    }
                }
            };

            Ok(Self { id, ptr, size })
        }
    }

    /// Get this shared memory segment as a mutable reference.
    ///
    /// # Safety
    ///
    /// One must ensure that no other processes are reading from or writing to this memory.
    unsafe fn as_mut(&mut self) -> &mut [i8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size) }
    }

    /// Get the size of this shared memory segment.
    fn size(&self) -> usize {
        self.size
    }

    /// Get the shared memory ID.
    fn id(&self) -> u32 {
        self.id as _
    }
}

impl Drop for ShmSegment {
    fn drop(&mut self) {
        unsafe {
            // Detach the shared memory segment.
            shmdt(self.ptr.as_ptr() as _);

            // Delete the shared memory segment.
            shmctl(self.id, IPC_RMID, null_mut());
        }
    }
}

impl Drop for X11Impl {
    fn drop(&mut self) {
        // If we used SHM, make sure it's detached from the server.
        if let Buffer::Shm(mut shm) = mem::replace(&mut self.buffer, Buffer::Wire(Vec::new())) {
            // If we were in the middle of processing a buffer, wait for it to finish.
            shm.finish_wait(&self.display.connection).ok();

            if let Some((segment, seg_id)) = shm.seg.take() {
                if let Ok(token) = self.display.connection.shm_detach(seg_id) {
                    token.ignore_error();
                }

                // Drop the segment.
                drop(segment);
            }
        }

        // Close the graphics context that we created.
        if let Ok(token) = self.display.connection.free_gc(self.gc) {
            token.ignore_error();
        }
    }
}

/// Test to see if SHM is available.
fn is_shm_available(c: &impl Connection) -> bool {
    // Create a small SHM segment.
    let seg = match ShmSegment::new(0x1000) {
        Ok(seg) => seg,
        Err(_) => return false,
    };

    // Attach and detach it.
    let seg_id = match c.generate_id() {
        Ok(id) => id,
        Err(_) => return false,
    };

    let (attach, detach) = {
        let attach = c.shm_attach(seg_id, seg.id(), false);
        let detach = c.shm_detach(seg_id);

        match (attach, detach) {
            (Ok(attach), Ok(detach)) => (attach, detach),
            _ => return false,
        }
    };

    // Check the replies.
    matches!((attach.check(), detach.check()), (Ok(()), Ok(())))
}

/// An error that can occur when pushing a buffer to the window.
#[derive(Debug)]
enum PushBufferError {
    /// We encountered an X11 error.
    X11(ReplyError),

    /// We exhausted the XID space.
    XidExhausted,

    /// A system error occurred while creating the shared memory segment.
    System(io::Error),
}

impl fmt::Display for PushBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X11(e) => write!(f, "X11 error: {}", e),
            Self::XidExhausted => write!(f, "XID space exhausted"),
            Self::System(e) => write!(f, "System error: {}", e),
        }
    }
}

impl std::error::Error for PushBufferError {}

impl From<ConnectionError> for PushBufferError {
    fn from(e: ConnectionError) -> Self {
        Self::X11(ReplyError::ConnectionError(e))
    }
}

impl From<ReplyError> for PushBufferError {
    fn from(e: ReplyError) -> Self {
        Self::X11(e)
    }
}

impl From<ReplyOrIdError> for PushBufferError {
    fn from(e: ReplyOrIdError) -> Self {
        match e {
            ReplyOrIdError::ConnectionError(e) => Self::X11(ReplyError::ConnectionError(e)),
            ReplyOrIdError::X11Error(e) => Self::X11(ReplyError::X11Error(e)),
            ReplyOrIdError::IdsExhausted => Self::XidExhausted,
        }
    }
}

impl From<io::Error> for PushBufferError {
    fn from(e: io::Error) -> Self {
        Self::System(e)
    }
}

/// Convenient wrapper to cast errors into SoftBufferError.
trait SwResultExt<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError>;
}

impl<T, E: std::error::Error + 'static> SwResultExt<T, E> for Result<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError> {
        self.map_err(|e| {
            SoftBufferError::PlatformError(Some(msg.into()), Some(Box::new(LibraryError(e))))
        })
    }
}

/// Convenient wrapper to cast errors into PushBufferError.
trait PushResultExt<T, E> {
    fn push_err(self) -> Result<T, PushBufferError>;
}

impl<T, E: Into<PushBufferError>> PushResultExt<T, E> for Result<T, E> {
    fn push_err(self) -> Result<T, PushBufferError> {
        self.map_err(Into::into)
    }
}

/// A wrapper around a library error.
///
/// This prevents `x11-dl` and `x11rb` from becoming public dependencies, since users cannot downcast
/// to this type.
struct LibraryError<E>(E);

impl<E: fmt::Debug> fmt::Debug for LibraryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<E: fmt::Display> fmt::Display for LibraryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for LibraryError<E> {}

/// Get the length that a slice needs to be to hold a buffer of the given dimensions.
#[inline(always)]
fn total_len(width: u16, height: u16) -> usize {
    let width: usize = width.into();
    let height: usize = height.into();

    width
        .checked_mul(height)
        .and_then(|len| len.checked_mul(4))
        .unwrap_or_else(|| panic!("Dimensions are too large: ({} x {})", width, height))
}
