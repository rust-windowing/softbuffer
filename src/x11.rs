//! Implementation of software buffering for X11.
//!
//! This module converts the input buffer into an XImage and then sends it over the wire to be
//! drawn. A more effective implementation would use shared memory instead of the wire. In
//! addition, we may also want to blit to a pixmap instead of a window.

use crate::SoftBufferError;
use nix::libc::{shmget, shmat, shmdt, shmctl, IPC_PRIVATE, IPC_RMID};
use raw_window_handle::{XcbDisplayHandle, XcbWindowHandle, XlibDisplayHandle, XlibWindowHandle};
use std::{fmt, io};
use std::ptr::{NonNull, null_mut};

use x11_dl::xlib::Display;
use x11_dl::xlib_xcb::Xlib_xcb;

use x11rb::connection::Connection;
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _, Gcontext, Window};
use x11rb::xcb_ffi::XCBConnection;

/// The handle to an X11 drawing context.
pub struct X11Impl {
    /// The handle to the XCB connection.
    connection: XCBConnection,

    /// The window to draw to.
    window: Window,

    /// The graphics context to use when drawing.
    gc: Gcontext,

    /// The depth (bits per pixel) of the drawing context.
    depth: u8,

    /// Information about SHM, if it is available.
    shm: Option<ShmInfo>
}

struct ShmInfo {

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

        // Start getting the depth of the window.
        let geometry_token = connection
            .get_geometry(window)
            .swbuf_err("Failed to send geometry request")?;

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

        Ok(Self {
            connection,
            window,
            gc,
            depth: geometry_reply.depth,
        })
    }

    pub(crate) unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        // Draw the image to the buffer.
        let result = self.connection.put_image(
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
        );

        match result {
            Err(e) => log::error!("Failed to draw image to window: {}", e),
            Ok(token) => token.ignore_error(),
        }
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

            // Get the pointer it maps to.
            let ptr = shmat(id, null_mut(), 0);
            let ptr = match NonNull::new(ptr as *mut i8) {
                Some(ptr) => ptr,
                None => {
                    shmctl(id, IPC_RMID, null_mut());
                    return Err(io::Error::last_os_error());
                }
            };

            Ok(Self { id, ptr, size })
        }
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
        // Close the graphics context that we created.
        if let Ok(token) = self.connection.free_gc(self.gc) {
            token.ignore_error();
        }
    }
}

/// Convenient wrapper to cast errors into SoftBufferError.
trait ResultExt<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError>;
}

impl<T, E: fmt::Debug + fmt::Display + 'static> ResultExt<T, E> for Result<T, E> {
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
