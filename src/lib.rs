#![doc = include_str!("../README.md")]
#![allow(clippy::needless_doctest_main)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate core;

mod backend_dispatch;
mod backend_interface;
mod backends;
mod error;
mod format;
mod pixel;
mod util;

use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

use self::backend_dispatch::*;
use self::backend_interface::*;
#[cfg(target_family = "wasm")]
pub use self::backends::web::SurfaceExtWeb;
use self::error::InitError;
pub use self::error::SoftBufferError;
pub use self::format::PixelFormat;
pub use self::pixel::Pixel;

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
    pub width: NonZeroU32,
    /// height
    pub height: NonZeroU32,
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
    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        if u32::MAX / 4 < width.get() {
            // Stride would be too large.
            return Err(SoftBufferError::SizeOutOfRange { width, height });
        }

        self.surface_impl.resize(width, height)?;

        Ok(())
    }

    /// Copies the window contents into a buffer.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On X11, the window must be visible.
    /// - On AppKit, UIKit, Redox and Wayland, this function is unimplemented.
    /// - On Web, this will fail if the content was supplied by
    ///   a different origin depending on the sites CORS rules.
    pub fn fetch(&mut self) -> Result<Vec<Pixel>, SoftBufferError> {
        self.surface_impl.fetch()
    }

    /// Return a [`Buffer`] that the next frame should be rendered into. The size must
    /// be set with [`Surface::resize`] first. The initial contents of the buffer may be zeroed, or
    /// may contain a previous frame. Call [`Buffer::age`] to determine this.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On DRM/KMS, there is no reliable and sound way to wait for the page flip to happen from within
    ///   `softbuffer`. Therefore it is the responsibility of the user to wait for the page flip before
    ///   sending another frame.
    pub fn buffer_mut(&mut self) -> Result<Buffer<'_>, SoftBufferError> {
        let mut buffer_impl = self.surface_impl.buffer_mut()?;

        debug_assert_eq!(
            buffer_impl.byte_stride().get() % 4,
            0,
            "stride must be a multiple of 4"
        );
        debug_assert_eq!(
            buffer_impl.height().get() as usize * buffer_impl.byte_stride().get() as usize,
            buffer_impl.pixels_mut().len() * 4,
            "buffer must be sized correctly"
        );
        debug_assert!(
            buffer_impl.width().get() * 4 <= buffer_impl.byte_stride().get(),
            "width * 4 must be less than or equal to stride"
        );

        Ok(Buffer {
            buffer_impl,
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
/// The buffer's contents is a [slice] of [`Pixel`]s, which depending on the backend may be a
/// mapping into shared memory accessible to the display server, so presentation doesn't require any
/// (client-side) copying.
///
/// This trusts the display server not to mutate the buffer, which could otherwise be unsound.
///
/// # Data representation
///
/// The buffer data is laid out in row-major order (split into horizontal lines), with origin in the
/// top-left corner. There is one [`Pixel`] (4 bytes) in the buffer for each pixel in the area to
/// draw.
///
/// # Reading buffer data
///
/// Reading from buffer data may perform very poorly, as the underlying storage of zero-copy
/// buffers, where implemented, may set options optimized for CPU writes, that allows them to bypass
/// certain caches and avoid cache pollution.
///
/// As such, when rendering, you should always set the pixel in its entirety:
///
/// ```
/// # use softbuffer::Pixel;
/// # let pixel = &mut Pixel::default();
/// # let (red, green, blue) = (0x11, 0x22, 0x33);
/// *pixel = Pixel::new_rgb(red, green, blue);
/// ```
///
/// Instead of e.g. something like:
///
/// ```
/// # use softbuffer::Pixel;
/// # let pixel = &mut Pixel::default();
/// # let (red, green, blue) = (0x11, 0x22, 0x33);
/// // DISCOURAGED!
/// *pixel = Pixel::default(); // Clear
/// pixel.r |= red;
/// pixel.g |= green;
/// pixel.b |= blue;
/// ```
///
/// To discourage reading from the buffer, `&self -> &[u8]` methods are intentionally not provided.
///
/// # Platform dependent behavior
///
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
    /// The number of bytes wide each row in the buffer is.
    ///
    /// On some platforms, the buffer is slightly larger than `width * height * 4`, usually for
    /// performance reasons to align each row such that they are never split across cache lines.
    ///
    /// In your code, prefer to use [`Buffer::pixel_rows`] (which handles this correctly), or
    /// failing that, make sure to chunk rows by the stride instead of the width.
    ///
    /// This is guaranteed to be `>= width * 4`, and is guaranteed to be a multiple of 4.
    #[doc(alias = "pitch")] // SDL and Direct3D
    #[doc(alias = "bytes_per_row")] // WebGPU
    #[doc(alias = "row_stride")]
    #[doc(alias = "stride")]
    pub fn byte_stride(&self) -> NonZeroU32 {
        self.buffer_impl.byte_stride()
    }

    /// The amount of pixels wide the buffer is.
    pub fn width(&self) -> NonZeroU32 {
        self.buffer_impl.width()
    }

    /// The amount of pixels tall the buffer is.
    pub fn height(&self) -> NonZeroU32 {
        self.buffer_impl.height()
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
    ///
    /// ## macOS / iOS
    ///
    /// On macOS/iOS/etc., this sets the [contents] of the underlying [`CALayer`], but doesn't yet
    /// actually commit those contents to the compositor; that is instead done automatically by
    /// QuartzCore at the end of the current iteration of the runloop. This synchronizes the
    /// contents with the rest of the window, which is important to avoid flickering when resizing.
    ///
    /// If you need to send the contents to the compositor immediately (might be useful when
    /// rendering from a separate thread or when using Softbuffer without the standard AppKit/UIKit
    /// runloop), you'll want to wrap this function in a [`CATransaction`].
    ///
    /// [contents]: https://developer.apple.com/documentation/quartzcore/calayer/contents?language=objc
    /// [`CALayer`]: https://developer.apple.com/documentation/quartzcore/calayer?language=objc
    /// [`CATransaction`]: https://developer.apple.com/documentation/quartzcore/catransaction?language=objc
    #[inline]
    pub fn present(self) -> Result<(), SoftBufferError> {
        // Damage the entire buffer.
        self.present_with_damage(&[Rect {
            x: 0,
            y: 0,
            width: NonZeroU32::MAX,
            height: NonZeroU32::MAX,
        }])
    }

    /// Presents buffer to the window, with damage regions.
    ///
    /// Damage regions that fall outside the surface are ignored.
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

/// Helper methods for writing to the buffer as RGBA pixel data.
impl Buffer<'_> {
    /// Get a mutable reference to the buffer's pixels.
    ///
    /// The size of the returned slice is `buffer.byte_stride() * buffer.height() / 4`.
    ///
    /// # Examples
    ///
    /// Clear the buffer with red.
    ///
    /// ```no_run
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
    /// buffer.pixels().fill(Pixel::new_rgb(0xff, 0x00, 0x00));
    /// ```
    ///
    /// Render to a slice of `[u8; 4]`s. This might be useful for library authors that want to
    /// provide a simple API that provides RGBX rendering.
    ///
    /// ```no_run
    /// use softbuffer::{Pixel, PixelFormat};
    ///
    /// // Assume the user controls the following rendering function:
    /// fn render(pixels: &mut [[u8; 4]], width: u32, height: u32) {
    ///     pixels.fill([0xff, 0xff, 0x00, 0x00]); // Yellow in RGBX
    /// }
    ///
    /// // Then we'd convert pixel data as follows:
    ///
    /// # let buffer: softbuffer::Buffer<'_> = todo!();
    /// # #[cfg(false)]
    /// let buffer = surface.buffer_mut();
    ///
    /// let width = buffer.width().get();
    /// let height = buffer.height().get();
    ///
    /// // Use fast, zero-copy implementation when possible, and fall back to slower version when not.
    /// if PixelFormat::Rgbx.is_default() && buffer.byte_stride().get() == width * 4 {
    ///     // SAFETY: `Pixel` can be reinterpreted as `[u8; 4]`.
    ///     let pixels = unsafe { std::mem::transmute::<&mut [Pixel], &mut [[u8; 4]]>(buffer.pixels()) };
    ///     // CORRECTNESS: We just checked that the format is RGBX, and that `stride == width * 4`.
    ///     render(pixels, width, height);
    /// } else {
    ///     // Render into temporary buffer.
    ///     let mut temporary = vec![[0; 4]; width as usize * height as usize];
    ///     render(&mut temporary, width, height);
    ///
    ///     // And copy from temporary buffer to actual pixel data.
    ///     for (tmp_row, actual_row) in temporary.chunks(width as usize).zip(buffer.pixel_rows()) {
    ///         for (tmp, actual) in tmp_row.iter().zip(actual_row) {
    ///             *actual = Pixel::new_rgb(tmp[0], tmp[1], tmp[2]);
    ///         }
    ///     }
    /// }
    ///
    /// buffer.present();
    /// ```
    pub fn pixels(&mut self) -> &mut [Pixel] {
        self.buffer_impl.pixels_mut()
    }

    /// Iterate over each row of pixels.
    ///
    /// Each slice returned from the iterator has a length of `buffer.byte_stride() / 4`.
    ///
    /// # Examples
    ///
    /// Fill each row with alternating black and white.
    ///
    /// ```no_run
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
    /// for (y, row) in buffer.pixel_rows().enumerate() {
    ///     if y % 2 == 0 {
    ///         row.fill(Pixel::new_rgb(0xff, 0xff, 0xff));
    ///     } else {
    ///         row.fill(Pixel::new_rgb(0x00, 0x00, 0x00));
    ///     }
    /// }
    /// ```
    ///
    /// Fill a red rectangle while skipping over regions that don't need to be modified.
    ///
    /// ```no_run
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
    /// let x = 100;
    /// let y = 200;
    /// let width = 10;
    /// let height = 20;
    ///
    /// for row in buffer.pixel_rows().skip(y).take(height) {
    ///     for pixel in row.iter_mut().skip(x).take(width) {
    ///         *pixel = Pixel::new_rgb(0xff, 0x00, 0x00);
    ///     }
    /// }
    /// ```
    ///
    /// Iterate over each pixel (similar to what the [`pixels_iter`] method does).
    ///
    /// [`pixels_iter`]: Self::pixels_iter
    ///
    /// ```no_run
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
    /// for (y, row) in buffer.pixel_rows().enumerate() {
    ///     for (x, pixel) in row.iter_mut().enumerate() {
    ///         *pixel = Pixel::new_rgb((x % 0xff) as u8, (y % 0xff) as u8, 0x00);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn pixel_rows(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut [Pixel]> + ExactSizeIterator {
        let pixel_stride = self.byte_stride().get() as usize / 4;
        let pixels = self.pixels();
        assert_eq!(pixels.len() % pixel_stride, 0, "must be multiple of stride");
        // NOTE: This won't panic because `pixel_stride` came from `NonZeroU32`
        pixels.chunks_mut(pixel_stride)
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
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
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
    ///         *pixel = Pixel::new_rgb(0xff, 0x00, 0x00);
    ///     } else {
    ///         // Outside rectangle.
    ///         *pixel = Pixel::new_rgb(0x00, 0x00, 0xff);
    ///     };
    /// }
    /// ```
    ///
    /// Iterate over the pixel data in reverse, and draw a red rectangle in the top-left corner.
    ///
    /// ```no_run
    /// # use softbuffer::{Buffer, Pixel};
    /// # let buffer: Buffer<'_> = unimplemented!();
    /// // Only reverses iteration order, x and y are still relative to the top-left corner.
    /// for (x, y, pixel) in buffer.pixels_iter().rev() {
    ///     if x <= 100 && y <= 100 {
    ///         *pixel = Pixel::new_rgb(0xff, 0x00, 0x00);
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn pixels_iter(&mut self) -> impl DoubleEndedIterator<Item = (u32, u32, &mut Pixel)> {
        self.pixel_rows().enumerate().flat_map(|(y, pixels)| {
            pixels
                .iter_mut()
                .enumerate()
                .map(move |(x, pixel)| (x as u32, y as u32, pixel))
        })
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
