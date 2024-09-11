#![doc = include_str!("../README.md")]
#![allow(clippy::needless_doctest_main)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

extern crate core;

mod backend_dispatch;
use backend_dispatch::*;
mod backend_interface;
use backend_interface::*;
use formats::RGBFormat;
mod backends;
mod error;
mod formats;
mod util;

use std::cell::Cell;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::ops;
use std::sync::Arc;

pub use backend_interface::{RGBA, RGBX};
use error::InitError;
pub use error::SoftBufferError;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

#[cfg(target_arch = "wasm32")]
pub use backends::web::SurfaceExtWeb;

/// An instance of this struct contains the platform-specific data that must be managed in order to
/// write to a window on that platform.
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

pub trait BufferReturn {
    type Output: RGBFormat + Copy;
    const ALPHA_MODE: bool;
}
pub enum WithoutAlpha {}

impl BufferReturn for WithoutAlpha {
    type Output = RGBX;
    const ALPHA_MODE: bool = false;
}
pub enum WithAlpha {}

impl BufferReturn for WithAlpha {
    type Output = RGBA;

    const ALPHA_MODE: bool = true;
}

/// A surface for drawing to a window with software buffers.
pub struct Surface<D, W, A = WithoutAlpha> {
    /// This is boxed so that `Surface` is the same size on every platform.
    surface_impl: Box<SurfaceDispatch<D, W, A>>,
    _marker: PhantomData<Cell<()>>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Surface<D, W, WithoutAlpha> {
    /// Creates a new surface for the context for the provided window.
    pub fn new(
        context: &Context<D>,
        window: W,
    ) -> Result<Surface<D, W, WithoutAlpha>, SoftBufferError> {
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
}

impl<D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> Surface<D, W, A> {
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

    /// Return a [`Buffer`] that the next frame should be rendered into. The size must
    /// be set with [`Surface::resize`] first. The initial contents of the buffer may be zeroed, or
    /// may contain a previous frame. Call [`Buffer::age`] to determine this.
    ///
    /// ## Platform Dependent Behavior
    ///
    /// - On DRM/KMS, there is no reliable and sound way to wait for the page flip to happen from within
    ///   `softbuffer`. Therefore it is the responsibility of the user to wait for the page flip before
    ///   sending another frame.
    pub fn buffer_mut(&mut self) -> Result<Buffer<'_, D, W, A>, SoftBufferError> {
        Ok(Buffer {
            buffer_impl: self.surface_impl.buffer_mut()?,
            _marker: PhantomData,
        })
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> Surface<D, W, WithAlpha> {
    /// Creates a new surface for the context for the provided window.
    pub fn new_with_alpha(
        context: &Context<D>,
        window: W,
    ) -> Result<Surface<D, W, WithAlpha>, SoftBufferError> {
        match SurfaceDispatch::new_with_alpha(window, &context.context_impl) {
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
}

impl<D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> AsRef<W> for Surface<D, W, A> {
    #[inline]
    fn as_ref(&self) -> &W {
        self.window()
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> HasWindowHandle
    for Surface<D, W, A>
{
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
pub struct Buffer<'a, D, W, A> {
    buffer_impl: BufferDispatch<'a, D, W, A>,
    _marker: PhantomData<(Arc<D>, Cell<()>)>,
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> Buffer<'a, D, W, A> {
    /// Is age is the number of frames ago this buffer was last presented. So if the value is
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

macro_rules! cast_to_format_helper {
    ($self:ident , $src:ident , $dst:ty, $func:ident , $to_func:ident , $from_func:ident) => {
        {
            let temp = $self.buffer_impl.pixels_mut();
            for element in temp.iter_mut() {
                unsafe {
                    let temp_as_concrete_type =
                        std::mem::transmute::<&mut u32, &mut <$src as BufferReturn>::Output>(element);
                    let temp_as_destination_type = temp_as_concrete_type.$to_func();
                    *element = std::mem::transmute(temp_as_destination_type);
                }
            }
            $func(temp);
            for element in temp.iter_mut() {
                unsafe {
                    let temp_as_concrete_type =
                        std::mem::transmute::<&mut u32, &mut $dst>(element);
                    let temp_as_destination_type =
                        &mut <$src as BufferReturn>::Output::$from_func(*temp_as_concrete_type);
                    *element = *std::mem::transmute::<&mut <$src as BufferReturn>::Output, &mut u32>(
                        temp_as_destination_type,
                    );
                }
            }
        }
    };
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> Buffer<'a, D, W, A> {
    /// Gets a ```&[u32]``` of the buffer of pixels
    /// The layout of the pixels is dependent on the platform that you are on
    /// It is recommended to deref pixels to a ```&[RGBA]``` ```&[RGBX]``` struct as that will automatically handle the differences across platforms for free
    /// If you need a ```&[u32]``` of a specific format, there are helper functions to get those, and conversions are automatic based on platform.
    /// If using the format for your native platform, there is no cost.
    pub fn pixels_platform_dependent(&self) -> &[u32] {
        self.buffer_impl.pixels()
    }
    /// Gets a ```&mut [u32]``` of the buffer of pixels
    /// The layout of the pixels is dependent on the platform that you are on
    /// It is recommended to deref pixels to a ```&mut [RGBA]``` ```&mut [RGBX]``` struct as that will automatically handle the differences across platforms for free
    /// If you need a ```&mut [u32]``` of a specific format, there are helper functions to get those, and conversions are automatic based on platform.
    /// If using the format for your native platform, there is no cost. 
    pub fn pixels_platform_dependent_mut(&mut self) -> &mut [u32] {
        self.buffer_impl.pixels_mut()
    }

    /// Access the platform dependent representation of the pixel buffer.
    /// Will return either ```&[RGBX]``` or ```&[RGBA]``` depending on if called on a surface with alpha enabled or not.
    /// This is the generally recommended method of accessing the pixel buffer as it is a zero cost abstraction, that 
    /// automatically handles any platform dependent ordering of the r,g,b,a fields for you.
    /// 
    /// Alternative to using Deref on buffer. 
    pub fn pixels_rgb(&mut self) -> &[<A as BufferReturn>::Output]{
        self.buffer_impl.pixels_rgb()
    }

    /// Access the platform dependent representation of the pixel buffer.
    /// Will return either ```&[RGBX]``` or ```&[RGBA]``` depending on if called on a surface with alpha enabled or not.
    /// This is the generally recommended method of accessing the pixel buffer as it is a zero cost abstraction, that 
    /// automatically handles any platform dependent ordering of the r,g,b,a fields for you.
    /// 
    /// Alternative to using Deref on buffer. 
    pub fn pixels_rgb_mut(&mut self) -> &mut[<A as BufferReturn>::Output]{
        self.buffer_impl.pixels_rgb_mut()
    }

    /// Gives a ```&mut [u32]``` slice in the RGBA u32 format.
    /// Endianness is adjusted based on platform automatically.
    /// If using the format for your native platform, there is no cost.
    /// Useful when using other crates that require a specific format.
    /// 
    /// This takes a closure that gives you the required ```&mut [u32]```.
    /// The closure is necessary because if conversion is required for your platform, we need to convert back to the platform native format before presenting to the buffer.
    pub fn pixel_u32slice_rgba<F: FnOnce(&mut [u32])>(&mut self, f: F) {
        cast_to_format_helper!(self,A,formats::RGBA,f,to_rgba_format,from_rgba_format)
    }

    /// Gives a ```&mut [u32]``` slice in the RGBA u8 format.
    /// If using the format for your native platform, there is no cost.
    /// Useful when using other crates that require a specific format.
    /// 
    /// This takes a closure that gives you the required ```&mut [u32]```.
    /// The closure is necessary because if conversion is required for your platform, we need to convert back to the platform native format before presenting to the buffer
    pub fn pixel_u8_slice_rgba<F: FnOnce(&mut [u8])>(&mut self, f: F) {
        let wrapper = |x: &mut [u32]|{
            f(bytemuck::cast_slice_mut(x))
        };
        cast_to_format_helper!(self,A,formats::RGBAu8,wrapper,to_rgba_u8_format,from_rgba_u8_format)
    }
}



#[cfg(feature = "compatibility")]
impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> ops::Deref
    for Buffer<'a, D, W, A>
{
    type Target = [u32];

    #[inline]
    fn deref(&self) -> &[u32] {
        self.buffer_impl.pixels()
    }
}

#[cfg(feature = "compatibility")]
impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> ops::DerefMut
    for Buffer<'a, D, W, A>
{
    #[inline]
    fn deref_mut(&mut self) -> &mut [u32] {
        self.buffer_impl.pixels_mut()
    }
}

#[cfg(not(feature = "compatibility"))]
impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> ops::Deref
    for Buffer<'a, D, W, A>
{
    type Target = [<A as BufferReturn>::Output];

    #[inline]
    fn deref(&self) -> &[<A as BufferReturn>::Output] {
        self.buffer_impl.pixels_rgb()
    }
}

#[cfg(not(feature = "compatibility"))]
impl<'a, D: HasDisplayHandle, W: HasWindowHandle, A: BufferReturn> ops::DerefMut
    for Buffer<'a, D, W, A>
{
    // type Target = [crate::RGBX];
    #[inline]
    fn deref_mut(&mut self) -> &mut [<A as BufferReturn>::Output] {
        self.buffer_impl.pixels_rgb_mut()
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
    is_send::<Buffer<'static, (), (), ()>>();

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
    /// __is_sync::<Buffer<'static, (), ()>>();
    /// ```
    fn __buffer_not_sync() {}
}
