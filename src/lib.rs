#![doc = include_str!("../README.md")]
#![allow(clippy::needless_doctest_main)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate core;

mod backend_dispatch;
use backend_dispatch::*;
mod backend_interface;
use backend_interface::*;
mod backends;
mod error;
mod util;

use std::cell::Cell;
use std::marker::PhantomData;
use std::ops;
use std::sync::Arc;

use error::InitError;
pub use error::SoftBufferError;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

#[cfg(target_family = "wasm")]
pub use backends::web::SurfaceExtWeb;

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform.
#[derive(Clone, Debug)]
pub struct Context<D> {
    /// The inner static dispatch object.
    context_impl: ContextDispatch<D>,

    /// This is Send+Sync IFF D is Send+Sync.
    _marker: PhantomData<Arc<D>>,
}

impl<D: HasDisplayHandle> Context<D> {
    /// Creates a new instance of this struct, using the provided display.
    pub fn new(display: D) -> Result<Self, SoftBufferError> {
        match ContextDispatch::new(display) {
            Ok(context_impl) => Ok(Self {
                context_impl,
                _marker: PhantomData,
            }),
            Err(InitError::Unsupported(display)) => {
                let raw = display.display_handle()?.as_raw();
                Err(SoftBufferError::UnsupportedDisplayPlatform {
                    human_readable_display_platform_name: display_handle_type_name(&raw),
                    display_handle: raw,
                })
            }
            Err(InitError::Failure(f)) => Err(f),
        }
    }
}

/// A rectangular region of the buffer coordinate space.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    /// x coordinate of top left corner
    pub x: u32,
    /// y coordinate of top left corner
    pub y: u32,
    /// width
    pub width: u32,
    /// height
    pub height: u32,
}

/// A surface for drawing to a window with software buffers.
#[derive(Debug)]
pub struct Surface<D, W> {
    /// This is boxed so that `Surface` is the same size on every platform.
    surface_impl: Box<SurfaceDispatch<D, W>>,
    _marker: PhantomData<Cell<()>>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Surface<D, W> {
    /// Creates a new surface for the context for the provided window.
    pub fn new(context: &Context<D>, window: W) -> Result<Self, SoftBufferError> {
        match SurfaceDispatch::new(window, &context.context_impl) {
            Ok(surface_dispatch) => Ok(Self {
                surface_impl: Box::new(surface_dispatch),
                _marker: PhantomData,
            }),
            Err(InitError::Unsupported(window)) => {
                let raw = window.window_handle()?.as_raw();
                Err(SoftBufferError::UnsupportedWindowPlatform {
                    human_readable_window_platform_name: window_handle_type_name(&raw),
                    human_readable_display_platform_name: context.context_impl.variant_name(),
                    window_handle: raw,
                })
            }
            Err(InitError::Failure(f)) => Err(f),
        }
    }

    /// Get a reference to the underlying window handle.
    pub fn window(&self) -> &W {
        self.surface_impl.window()
    }

    /// Set the size of the buffer that will be returned by [`Surface::buffer_mut`].
    ///
    /// If the size of the buffer does not match the size of the window, the buffer is drawn
    /// in the upper-left corner of the window. It is recommended in most production use cases
    /// to have the buffer fill the entire window. Use your windowing library to find the size
    /// of the window.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SoftBufferError> {
        self.surface_impl.resize(width, height)
    }

    /// Copies the window contents into a buffer.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On X11, the window must be visible.
    /// - On AppKit, UIKit, Redox and Wayland, this function is unimplemented.
    /// - On Web, this will fail if the content was supplied by
    ///   a different origin depending on the sites CORS rules.
    pub fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        self.surface_impl.fetch()
    }

    /// Return a [`Buffer`] that the next frame should be rendered into.
    ///
    /// The buffer is initially empty, you'll want to set an appropriate size for it with
    /// [`Surface::resize`].
    ///
    /// The initial contents of the buffer may be zeroed, or may contain a previous frame. Call
    /// [`Buffer::age`] to determine this.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On DRM/KMS, there is no reliable and sound way to wait for the page flip to happen from within
    ///   `softbuffer`. Therefore it is the responsibility of the user to wait for the page flip before
    ///   sending another frame.
    pub fn buffer_mut(&mut self) -> Result<Buffer<'_>, SoftBufferError> {
        Ok(Buffer {
            buffer_impl: self.surface_impl.buffer_mut()?,
            _marker: PhantomData,
        })
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> AsRef<W> for Surface<D, W> {
    #[inline]
    fn as_ref(&self) -> &W {
        self.window()
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> HasWindowHandle for Surface<D, W> {
    #[inline]
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window().window_handle()
    }
}

/// A buffer that can be written to by the CPU and presented to the window.
///
/// This derefs to a `[u32]`, which depending on the backend may be a mapping into shared memory
/// accessible to the display server, so presentation doesn't require any (client-side) copying.
///
/// This trusts the display server not to mutate the buffer, which could otherwise be unsound.
///
/// # Data representation
///
/// The format of the buffer is as follows. There is one `u32` in the buffer for each pixel in
/// the area to draw. The first entry is the upper-left most pixel. The second is one to the right
/// etc. (Row-major top to bottom left to right one `u32` per pixel). Within each `u32` the highest
/// order 8 bits are to be set to 0. The next highest order 8 bits are the red channel, then the
/// green channel, and then the blue channel in the lowest-order 8 bits. See the examples for
/// one way to build this format using bitwise operations.
///
/// --------
///
/// Pixel format (`u32`):
///
/// 00000000RRRRRRRRGGGGGGGGBBBBBBBB
///
/// 0: Bit is 0
/// R: Red channel
/// G: Green channel
/// B: Blue channel
///
/// # Platform dependent behavior
/// No-copy presentation is currently supported on:
/// - Wayland
/// - X, when XShm is available
/// - Win32
/// - Orbital, when buffer size matches window size
///
/// Currently [`Buffer::present`] must block copying image data on:
/// - Web
/// - AppKit
/// - UIKit
///
/// Buffer copies an channel swizzling happen on:
/// - Android
#[derive(Debug)]
pub struct Buffer<'a> {
    buffer_impl: BufferDispatch<'a>,
    _marker: PhantomData<Cell<()>>,
}

impl Buffer<'_> {
    /// The amount of pixels wide the buffer is.
    pub fn width(&self) -> u32 {
        let width = self.buffer_impl.width();
        debug_assert_eq!(
            width as usize * self.buffer_impl.height() as usize,
            self.len(),
            "buffer must be sized correctly"
        );
        width
    }

    /// The amount of pixels tall the buffer is.
    pub fn height(&self) -> u32 {
        let height = self.buffer_impl.height();
        debug_assert_eq!(
            height as usize * self.buffer_impl.width() as usize,
            self.len(),
            "buffer must be sized correctly"
        );
        height
    }

    /// `age` is the number of frames ago this buffer was last presented. So if the value is
    /// `1`, it is the same as the last frame, and if it is `2`, it is the same as the frame
    /// before that (for backends using double buffering). If the value is `0`, it is a new
    /// buffer that has unspecified contents.
    ///
    /// This can be used to update only a portion of the buffer.
    pub fn age(&self) -> u8 {
        self.buffer_impl.age()
    }

    /// Presents buffer to the window.
    ///
    /// # Platform dependent behavior
    ///
    /// ## Wayland
    ///
    /// On Wayland, calling this function may send requests to the underlying `wl_surface`. The
    /// graphics context may issue `wl_surface.attach`, `wl_surface.damage`, `wl_surface.damage_buffer`
    /// and `wl_surface.commit` requests when presenting the buffer.
    ///
    /// If the caller wishes to synchronize other surface/window changes, such requests must be sent to the
    /// Wayland compositor before calling this function.
    pub fn present(self) -> Result<(), SoftBufferError> {
        self.buffer_impl.present()
    }

    /// Presents buffer to the window, with damage regions.
    ///
    /// # Platform dependent behavior
    ///
    /// Supported on:
    /// - Wayland
    /// - X, when XShm is available
    /// - Win32
    /// - Web
    ///
    /// Otherwise this is equivalent to [`Self::present`].
    pub fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.buffer_impl.present_with_damage(damage)
    }
}

/// Pixel helper accessors.
impl Buffer<'_> {
    /// Iterate over each row of pixels.
    ///
    /// Each slice returned from the iterator has a length of `buffer.width()`.
    ///
    /// # Examples
    ///
    /// Fill each row with alternating black and white.
    ///
    /// ```no_run
    /// # let buffer: softbuffer::Buffer<'_> = unimplemented!();
    /// for (y, row) in buffer.pixel_rows().enumerate() {
    ///     if y % 2 == 0 {
    ///         row.fill(0x00ffffff);
    ///     } else {
    ///         row.fill(0x00000000);
    ///     }
    /// }
    /// ```
    ///
    /// Fill a red rectangle while skipping over regions that don't need to be modified.
    ///
    /// ```no_run
    /// # let buffer: softbuffer::Buffer<'_> = unimplemented!();
    /// let x = 100;
    /// let y = 200;
    /// let width = 10;
    /// let height = 20;
    ///
    /// for row in buffer.pixel_rows().skip(y).take(height) {
    ///     for pixel in row.iter_mut().skip(x).take(width) {
    ///         *pixel = 0x00ff0000;
    ///     }
    /// }
    /// ```
    ///
    /// Iterate over each pixel (similar to what the [`pixels_iter`] method does).
    ///
    /// [`pixels_iter`]: Self::pixels_iter
    ///
    /// ```no_run
    /// # let buffer: softbuffer::Buffer<'_> = unimplemented!();
    /// # let pixel_value = |x, y| 0x00000000;
    /// for (y, row) in buffer.pixel_rows().enumerate() {
    ///     for (x, pixel) in row.iter_mut().enumerate() {
    ///         *pixel = pixel_value(x, y);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn pixel_rows(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut [u32]> + ExactSizeIterator {
        let width = self.width() as usize;
        let pixels = self.buffer_impl.pixels_mut();
        assert_eq!(pixels.len() % width, 0, "buffer must be multiple of width");
        // Clamp width to be at least 1 - when the width is 0, the pixel buffer is empty.
        pixels.chunks_mut(width.max(1))
    }

    /// Iterate over each pixel in the data.
    ///
    /// The returned iterator contains the `x` and `y` coordinates and a mutable reference to the
    /// pixel at that position.
    ///
    /// # Examples
    ///
    /// Draw a red rectangle with a margin of 10 pixels, and fill the background with blue.
    ///
    /// ```no_run
    /// # let buffer: softbuffer::Buffer<'_> = unimplemented!();
    /// let width = buffer.width().get();
    /// let height = buffer.height().get();
    /// let left = 10;
    /// let top = 10;
    /// let right = width.saturating_sub(10);
    /// let bottom = height.saturating_sub(10);
    ///
    /// for (x, y, pixel) in buffer.pixels_iter() {
    ///     if (left..=right).contains(&x) && (top..=bottom).contains(&y) {
    ///         // Inside rectangle.
    ///         *pixel = 0x00ff0000;
    ///     } else {
    ///         // Outside rectangle.
    ///         *pixel = 0x000000ff;
    ///     };
    /// }
    /// ```
    ///
    /// Iterate over the pixel data in reverse, and draw a red rectangle in the top-left corner.
    ///
    /// ```no_run
    /// # let buffer: softbuffer::Buffer<'_> = unimplemented!();
    /// // Only reverses iteration order, x and y are still relative to the top-left corner.
    /// for (x, y, pixel) in buffer.pixels_iter().rev() {
    ///     if x <= 100 && y <= 100 {
    ///         *pixel = 0x00ff0000;
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn pixels_iter(&mut self) -> impl DoubleEndedIterator<Item = (u32, u32, &mut u32)> {
        self.pixel_rows().enumerate().flat_map(|(y, pixels)| {
            pixels
                .iter_mut()
                .enumerate()
                .map(move |(x, pixel)| (x as u32, y as u32, pixel))
        })
    }
}

impl ops::Deref for Buffer<'_> {
    type Target = [u32];

    #[inline]
    fn deref(&self) -> &[u32] {
        self.buffer_impl.pixels()
    }
}

impl ops::DerefMut for Buffer<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u32] {
        self.buffer_impl.pixels_mut()
    }
}

/// There is no display handle.
#[derive(Debug)]
#[allow(dead_code)]
pub struct NoDisplayHandle(core::convert::Infallible);

impl HasDisplayHandle for NoDisplayHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        match self.0 {}
    }
}

/// There is no window handle.
#[derive(Debug)]
pub struct NoWindowHandle(());

impl HasWindowHandle for NoWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

fn window_handle_type_name(handle: &RawWindowHandle) -> &'static str {
    match handle {
        RawWindowHandle::Xlib(_) => "Xlib",
        RawWindowHandle::Win32(_) => "Win32",
        RawWindowHandle::WinRt(_) => "WinRt",
        RawWindowHandle::Web(_) => "Web",
        RawWindowHandle::Wayland(_) => "Wayland",
        RawWindowHandle::AndroidNdk(_) => "AndroidNdk",
        RawWindowHandle::AppKit(_) => "AppKit",
        RawWindowHandle::Orbital(_) => "Orbital",
        RawWindowHandle::UiKit(_) => "UiKit",
        RawWindowHandle::Xcb(_) => "XCB",
        RawWindowHandle::Drm(_) => "DRM",
        RawWindowHandle::Gbm(_) => "GBM",
        RawWindowHandle::Haiku(_) => "Haiku",
        _ => "Unknown Name", //don't completely fail to compile if there is a new raw window handle type that's added at some point
    }
}

fn display_handle_type_name(handle: &RawDisplayHandle) -> &'static str {
    match handle {
        RawDisplayHandle::Xlib(_) => "Xlib",
        RawDisplayHandle::Web(_) => "Web",
        RawDisplayHandle::Wayland(_) => "Wayland",
        RawDisplayHandle::AppKit(_) => "AppKit",
        RawDisplayHandle::Orbital(_) => "Orbital",
        RawDisplayHandle::UiKit(_) => "UiKit",
        RawDisplayHandle::Xcb(_) => "XCB",
        RawDisplayHandle::Drm(_) => "DRM",
        RawDisplayHandle::Gbm(_) => "GBM",
        RawDisplayHandle::Haiku(_) => "Haiku",
        RawDisplayHandle::Windows(_) => "Windows",
        RawDisplayHandle::Android(_) => "Android",
        _ => "Unknown Name", //don't completely fail to compile if there is a new raw window handle type that's added at some point
    }
}

#[cfg(not(target_family = "wasm"))]
fn __assert_send() {
    fn is_send<T: Send>() {}
    fn is_sync<T: Sync>() {}

    is_send::<Context<()>>();
    is_sync::<Context<()>>();
    is_send::<Surface<(), ()>>();
    is_send::<Buffer<'static>>();

    /// ```compile_fail
    /// use softbuffer::Surface;
    ///
    /// fn __is_sync<T: Sync>() {}
    /// __is_sync::<Surface<(), ()>>();
    /// ```
    fn __surface_not_sync() {}
    /// ```compile_fail
    /// use softbuffer::Buffer;
    ///
    /// fn __is_sync<T: Sync>() {}
    /// __is_sync::<Buffer<'static>>();
    /// ```
    fn __buffer_not_sync() {}
}
