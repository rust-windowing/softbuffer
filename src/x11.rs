//! Implementation of software buffering for X11.
//! 
//! This module converts the input buffer into an XImage and then sends it over the wire to be
//! drawn. A more effective implementation would use shared memory instead of the wire. In
//! addition, we may also want to blit to a pixmap instead of a window.

use crate::{GraphicsContextImpl, SwBufError};
use raw_window_handle::{XlibDisplayHandle, XlibWindowHandle};
use std::os::raw::{c_char, c_uint};
use x11_dl::xlib::{Display, Visual, Xlib, ZPixmap, GC};

/// The handle to an X11 drawing context.
pub struct X11Impl {
    /// The window handle.
    window_handle: XlibWindowHandle,

    /// The display handle.
    display_handle: XlibDisplayHandle,

    /// Reference to the X11 shared library.
    lib: Xlib,

    /// The graphics context for drawing.
    gc: GC,

    /// Information about the screen to use for drawing.
    visual: *mut Visual,

    /// The depth (bits per pixel) of the drawing context.
    depth: i32,
}

impl X11Impl {
    /// Create a new `X11Impl` from a `XlibWindowHandle` and `XlibDisplayHandle`.
    /// 
    /// # Safety
    /// 
    /// The `XlibWindowHandle` and `XlibDisplayHandle` must be valid.
    pub unsafe fn new(
        window_handle: XlibWindowHandle,
        display_handle: XlibDisplayHandle,
    ) -> Result<Self, SwBufError> {
        // Try to open the X11 shared library.
        let lib = match Xlib::open() {
            Ok(lib) => lib,
            Err(e) => {
                return Err(SwBufError::PlatformError(
                    Some("Failed to open Xlib".into()),
                    Some(Box::new(e)),
                ))
            }
        };

        // Validate the handles to ensure that they aren't incomplete.
        if display_handle.display.is_null() {
            return Err(SwBufError::IncompleteDisplayHandle);
        }

        if window_handle.window.is_null() {
            return Err(SwBufError::IncompleteWindowHandle);
        }

        // Get the screen number from the handle.
        // NOTE: By default, XlibDisplayHandle sets the screen number to 0. If we see a zero,
        // it could mean either screen index zero, or that the screen number was not set. We
        // can't tell which, so we'll just assume that the screen number was not set.
        let screen = match display_handle.screen {
            0 => (lib.XDefaultScreen)(display_handle.display as *mut Display),
            screen => screen,
        };

        // Use the default graphics context, visual and depth for this screen.
        let gc = (lib.XDefaultGC)(display_handle.display as *mut Display, screen);
        let visual = (lib.XDefaultVisual)(display_handle.display as *mut Display, screen);
        let depth = (lib.XDefaultDepth)(display_handle.display as *mut Display, screen);

        Ok(Self {
            window_handle,
            display_handle,
            lib,
            gc,
            visual,
            depth,
        })
    }
}

impl GraphicsContextImpl for X11Impl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        // Create the image from the buffer.
        let image = (self.lib.XCreateImage)(
            self.display_handle.display as *mut Display,
            self.visual,
            self.depth as u32,
            ZPixmap,
            0,
            (buffer.as_ptr()) as *mut c_char,
            width as u32,
            height as u32,
            32,
            (width * 4) as i32,
        );

        // Draw the image to the window.
        (self.lib.XPutImage)(
            self.display_handle.display as *mut Display,
            self.window_handle.window,
            self.gc,
            image,
            0,
            0,
            0,
            0,
            width as c_uint,
            height as c_uint,
        );

        // Delete the image data.
        (*image).data = std::ptr::null_mut();
        (self.lib.XDestroyImage)(image);
    }
}
