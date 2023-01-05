//! Implementation of software buffering for X11.
//!
//! This module converts the input buffer into an XImage and then sends it over the wire to be
//! drawn by the X server. The SHM extension is used if available.

#![allow(clippy::uninlined_format_args)]

use crate::SoftBufferError;
use nix::libc::{shmat, shmctl, shmdt, shmget, IPC_PRIVATE, IPC_RMID};
use raw_window_handle::{XcbDisplayHandle, XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle};
use std::ptr::{null_mut, NonNull};
use std::{fmt, io};

use x11_dl::xlib::Display;
use x11_dl::xlib_xcb::Xlib_xcb;

use x11rb::connection::{Connection, RequestConnection, SequenceNumber};
use x11rb::cookie::Cookie;
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::xcb_ffi::XCBConnection;

/// The handle to an X11 drawing context.
pub struct X11Impl {
    /// The handle to the XCB connection.
    connection: XCBConnection,

    /// The window to draw to.
    window: xproto::Window,

    /// The graphics context to use when drawing.
    gc: xproto::Gcontext,

    /// The depth (bits per pixel) of the drawing context.
    depth: u8,

    /// Information about SHM, if it is available.
    shm: Option<ShmInfo>,
}

struct ShmInfo {
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
        display_handle: XlibDisplayHandle,
    ) -> Result<Self, SoftBufferError> {
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

        let mut xcb_window_handle = XcbWindowHandle::empty();
        xcb_window_handle.window = window_handle.window as _;
        xcb_window_handle.visual_id = window_handle.visual_id as _;

        // SAFETY: If the user passed in valid Xlib handles, then these are valid XCB handles.
        unsafe { Self::from_xcb(xcb_window_handle, xcb_display_handle) }
    }

    /// Create a new `X11Impl` from a `XcbWindowHandle` and `XcbDisplayHandle`.
    ///
    /// # Safety
    ///
    /// The `XcbWindowHandle` and `XcbDisplayHandle` must be valid.
    pub(crate) unsafe fn from_xcb(
        window_handle: XcbWindowHandle,
        display_handle: XcbDisplayHandle,
    ) -> Result<Self, SoftBufferError> {
        // Check that the handles are valid.
        if display_handle.connection.is_null() {
            return Err(SoftBufferError::IncompleteDisplayHandle);
        }

        if window_handle.window == 0 {
            return Err(SoftBufferError::IncompleteWindowHandle);
        }

        // Wrap the display handle in an x11rb connection.
        // SAFETY: We don't own the connection, so don't drop it. We also assert that the connection is valid.
        let connection = {
            let result =
                unsafe { XCBConnection::from_raw_xcb_connection(display_handle.connection, false) };

            result.swbuf_err("Failed to wrap XCB connection")?
        };

        let window = window_handle.window;

        // Run in parallel: start getting the window depth and the SHM extension.
        let geometry_token = connection
            .get_geometry(window)
            .swbuf_err("Failed to send geometry request")?;
        connection
            .prefetch_extension_information(shm::X11_EXTENSION_NAME)
            .swbuf_err("Failed to send SHM query request")?;

        // Create a new graphics context to draw to.
        let gc = connection
            .generate_id()
            .swbuf_err("Failed to generate GC ID")?;
        connection
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
        let shm_info = {
            let present = is_shm_available(&connection);

            if present {
                // SHM is available.
                Some(ShmInfo {
                    seg: None,
                    done_processing: None,
                })
            } else {
                None
            }
        };

        Ok(Self {
            connection,
            window,
            gc,
            depth: geometry_reply.depth,
            shm: shm_info,
        })
    }

    pub(crate) unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        // Draw the image to the buffer.
        let result = unsafe { self.set_buffer_shm(buffer, width, height) }.and_then(|had_shm| {
            if had_shm {
                Ok(())
            } else {
                log::debug!("Falling back to non-SHM method");
                self.set_buffer_fallback(buffer, width, height)
            }
        });

        if let Err(e) = result {
            log::error!("Failed to draw image to window: {}", e);
        }
    }

    /// Put the given buffer into the window using the SHM method.
    ///
    /// Returns `false` if SHM is not available.
    ///
    /// # Safety
    ///
    /// The buffer's length must be `width * height`.
    unsafe fn set_buffer_shm(
        &mut self,
        buffer: &[u32],
        width: u16,
        height: u16,
    ) -> Result<bool, PushBufferError> {
        let shm_info = match self.shm {
            Some(ref mut info) => info,
            None => return Ok(false),
        };

        // If the X server is still processing the last image, wait for it to finish.
        shm_info.finish_wait(&self.connection)?;

        // Get the SHM segment to use.
        let necessary_size = (width as usize) * (height as usize) * 4;
        let (segment, segment_id) = shm_info.segment(&self.connection, necessary_size)?;

        // Copy the buffer into the segment.
        // SAFETY: The buffer is properly sized and we've ensured that the X server isn't reading from it.
        unsafe {
            segment.copy(buffer);
        }

        // Put the image into the window.
        self.connection
            .shm_put_image(
                self.window,
                self.gc,
                width,
                height,
                0,
                0,
                width,
                height,
                0,
                0,
                self.depth,
                xproto::ImageFormat::Z_PIXMAP.into(),
                false,
                segment_id,
                0,
            )?
            .ignore_error();

        // Send a short request to act as a notification for when the X server is done processing the image.
        shm_info.begin_wait(&self.connection)?;

        Ok(true)
    }

    /// Put the given buffer into the window using the fallback wire transfer method.
    fn set_buffer_fallback(
        &mut self,
        buffer: &[u32],
        width: u16,
        height: u16,
    ) -> Result<(), PushBufferError> {
        self.connection
            .put_image(
                xproto::ImageFormat::Z_PIXMAP,
                self.window,
                self.gc,
                width,
                height,
                0,
                0,
                0,
                self.depth,
                bytemuck::cast_slice(buffer),
            )?
            .ignore_error();

        Ok(())
    }
}

impl ShmInfo {
    /// Allocate a new `ShmSegment` of the given size.
    fn segment(
        &mut self,
        conn: &impl Connection,
        size: usize,
    ) -> Result<(&mut ShmSegment, shm::Seg), PushBufferError> {
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

        // Get the segment and ID.
        Ok(self
            .seg
            .as_mut()
            .map(|(ref mut seg, id)| (seg, *id))
            .unwrap())
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

    /// Copy data into this shared memory segment.
    ///
    /// # Safety
    ///
    /// This function assumes that the size of `self`'s buffer is larger than or equal to `data.len()`.
    /// In addition, no other processes should be reading from or writing to this memory.
    unsafe fn copy<T: bytemuck::NoUninit>(&mut self, data: &[T]) {
        debug_assert!(data.len() * std::mem::size_of::<T>() <= self.size,);
        let incoming_data = bytemuck::cast_slice::<_, u8>(data);

        unsafe {
            std::ptr::copy_nonoverlapping(
                incoming_data.as_ptr(),
                self.ptr.as_ptr() as *mut u8,
                incoming_data.len(),
            )
        }
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
        if let Some(mut shm) = self.shm.take() {
            // If we were in the middle of processing a buffer, wait for it to finish.
            shm.finish_wait(&self.connection).ok();

            if let Some((segment, seg_id)) = shm.seg.take() {
                if let Ok(token) = self.connection.shm_detach(seg_id) {
                    token.ignore_error();
                }

                // Drop the segment.
                drop(segment);
            }
        }

        // Close the graphics context that we created.
        if let Ok(token) = self.connection.free_gc(self.gc) {
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
trait ResultExt<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError>;
}

impl<T, E: std::error::Error + 'static> ResultExt<T, E> for Result<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError> {
        self.map_err(|e| {
            SoftBufferError::PlatformError(Some(msg.into()), Some(Box::new(LibraryError(e))))
        })
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
