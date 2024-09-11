//! Interface implemented by backends

use crate::{formats::RGBFormat, BufferReturn, InitError, Rect, SoftBufferError};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::{fmt::Debug, num::NonZeroU32};
use num::cast::AsPrimitive;

pub(crate) trait ContextInterface<D: HasDisplayHandle + ?Sized> {
    fn new(display: D) -> Result<Self, InitError<D>>
    where
        D: Sized,
        Self: Sized;
}

pub(crate) trait SurfaceInterface<D: HasDisplayHandle + ?Sized, W: HasWindowHandle + ?Sized, A: BufferReturn> {
    type Context: ContextInterface<D>;
    type Buffer<'a>: BufferInterface<A>
    where
        Self: 'a;

    fn new(window: W, context: &Self::Context) -> Result<Self, InitError<W>>
    where
        W: Sized,
        Self: Sized;
    fn new_with_alpha(window: W, context: &Self::Context) -> Result<Self, InitError<W>>
    where
        W: Sized,
        Self: Sized;
    /// Get the inner window handle.
    fn window(&self) -> &W;
    /// Resize the internal buffer to the given width and height.
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError>;
    /// Get a mutable reference to the buffer.
    fn buffer_mut(&mut self) -> Result<Self::Buffer<'_>, SoftBufferError>;
    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

pub(crate) trait BufferInterface<A: BufferReturn> {
    // #[deprecated = "Left for backwards compatibility. Will panic in the future. Switch to using the pixels_rgb or pixels_rgba methods for better cross platform portability"]
    fn pixels(&self) -> &[u32];
    // #[deprecated = "Left for backwards compatibility. Will panic in the future. Switch to using the pixels_rgb_mut or pixels_rgba_mut methods for better cross platform portability"]
    fn pixels_mut(&mut self) -> &mut [u32];
    fn pixels_rgb(&self) -> &[<A as BufferReturn>::Output];
    fn pixels_rgb_mut(&mut self) -> &mut[<A as BufferReturn>::Output];
    fn age(&self) -> u8;
    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError>;
    fn present(self) -> Result<(), SoftBufferError>;
}



macro_rules! define_rgbx_little_endian {
    (
        $(
            $(#[$attr:meta])*
            $first_vis:vis $first:ident,$second_vis:vis $second:ident,$third_vis:vis $third:ident,$forth_vis:vis $forth:ident
        )*
    ) => {
        $(
            $(#[$attr])*
            #[repr(C)]
            #[derive(Copy,Clone)]
            pub struct RGBX{
                $forth_vis $forth: u8,
                $third_vis $third: u8,
                $second_vis $second: u8,
                $first_vis $first: u8,
            }
        )*
    };
}

macro_rules! define_rgba_little_endian {
    (
        $(
            $(#[$attr:meta])*
            $first_vis:vis $first:ident,$second_vis:vis $second:ident,$third_vis:vis $third:ident,$forth_vis:vis $forth:ident
        )*
    ) => {
        $(
            $(#[$attr])*
            #[repr(C)]
            #[derive(Copy,Clone)]
            pub struct RGBA{
                $forth_vis $forth: u8,
                $third_vis $third: u8,
                $second_vis $second: u8,
                $first_vis $first: u8,
            }
        )*
    };
}

define_rgbx_little_endian!{
    #[cfg(x11_platform)]
    x,pub r,pub g,pub b
    #[cfg(wayland_platform)]
    x,pub r,pub g,pub b
    #[cfg(kms_platform)]
    x,pub r,pub g,pub b
    #[cfg(target_os = "windows")]
    x,pub r,pub g,pub b
    #[cfg(target_vendor = "apple")]
    x,pub r,pub g,pub b
    #[cfg(target_arch = "wasm32")]
    x,pub r,pub g,pub b
    #[cfg(target_os = "redox")]
    x,pub r,pub g,pub b
}

define_rgba_little_endian!{
    #[cfg(x11_platform)]
    pub a,pub r,pub g,pub b
    #[cfg(wayland_platform)]
    pub a,pub r,pub g,pub b
    #[cfg(kms_platform)]
    pub a,pub r,pub g,pub b
    #[cfg(target_os = "windows")]
    pub a,pub r,pub g,pub b
    #[cfg(target_vendor = "apple")]
    pub a,pub r,pub g,pub b
    #[cfg(target_arch = "wasm32")]
    pub a,pub r,pub g,pub b
    #[cfg(target_os = "redox")]
    pub a,pub r,pub g,pub b
}

impl RGBX{
    #[inline]
    /// Creates new RGBX from r,g,b values.
    /// Takes any primitive value that can be converted to a u8 using the ```as``` keyword
    /// If the value is greater than the u8::MAX the function will return an error 
    pub fn new<T>(r: T,g: T,b: T) -> Result<Self,SoftBufferError>
    where 
        T: AsPrimitive<u8> + std::cmp::PartialOrd<T>,
        u8: AsPrimitive<T>
    {
        let MAX_U8 = 255.as_();
        if r > MAX_U8 || g > MAX_U8 || b > MAX_U8{
            Err(SoftBufferError::PrimitiveOutsideOfU8Range)
        }else{
            Ok(Self { r: r.as_(), g: g.as_(), b: b.as_(), x: 0 })
        }
    }

    /// Creates new RGBX from r,g,b values.
    /// Takes any primitive value that can be converted to a u8 using the ```as``` keyword
    /// Unlike ```RGBX::new``` this function does not care if the value you provide is greater than u8. It will silently ignore any higher bits, taking only the last 8 bits.
    #[inline]
    pub fn new_unchecked<T>(r: T,g: T,b: T) -> Self
    where 
        T: AsPrimitive<u8>
    {
        Self { r: r.as_(), g: g.as_(), b: b.as_(), x: 255 }
    }

    // pub fn new_from_u32(u32: u32) -> Self{
    //     todo!()
    // }

    // pub fn as_u32(&self) -> &u32{
    //     unsafe{std::mem::transmute(self)}
    // }
}

impl RGBA{
    #[inline]
    /// Creates new RGBX from r,g,b values.
    /// Takes any primitive value that can be converted to a u8 using the ```as``` keyword
    /// If the value is greater than the u8::MAX the function will return an error 
    pub fn new<T>(r: T,g: T,b: T,a: T) -> Result<Self,SoftBufferError>
    where 
        T: AsPrimitive<u8> + std::cmp::PartialOrd<T>,
        u8: AsPrimitive<T>
    {
        let max_u8 = 255.as_();
        if r > max_u8 || g > max_u8 || b > max_u8 || a > max_u8{
            Err(SoftBufferError::PrimitiveOutsideOfU8Range)
        }else{
            Ok(Self { r: r.as_(), g: g.as_(), b: b.as_(), a: a.as_() })
        }
    }

    /// Creates new RGBX from r,g,b values.
    /// Takes any primitive value that can be converted to a u8 using the ```as``` keyword
    /// Unlike ```RGBX::new``` this function does not care if the value you provide is greater than u8. It will silently ignore any higher bits, taking only the last 8 bits.
    #[inline]
    pub fn new_unchecked<T>(r: T,g: T,b: T, a: T) -> Self
    where 
        T: AsPrimitive<u8>
    {
        Self { r: r.as_(), g: g.as_(), b: b.as_(), a: a.as_() }
    }
}

//TODO, change this to be a different impl based on platform
impl RGBFormat for RGBA{
    fn to_rgba_format(self) -> crate::formats::RGBA {
        crate::formats::RGBA{
            a: self.a,
            b: self.b,
            g: self.g,
            r: self.r,
        }
    }
    
    fn from_rgba_format(rgba: crate::formats::RGBA) -> Self {
        Self{
            b: rgba.b,
            g: rgba.g,
            r: rgba.r,
            a: rgba.a,
        }
    }
    
    fn to_rgba_u8_format(self) -> crate::formats::RGBAu8 {
        crate::formats::RGBAu8{
            a: self.a,
            b: self.b,
            g: self.g,
            r: self.r,
        }
    }
    
    fn from_rgba_u8_format(rgba: crate::formats::RGBAu8) -> Self {
        Self{
            b: rgba.b,
            g: rgba.g,
            r: rgba.r,
            a: rgba.a,
        }
    }
    
}

impl RGBFormat for RGBX{
    fn to_rgba_format(self) -> crate::formats::RGBA {
        crate::formats::RGBA{
            a: self.x,
            b: self.b,
            g: self.g,
            r: self.r,
        }
    }
    
    fn from_rgba_format(rgba: crate::formats::RGBA) -> Self {
        Self{
            b: rgba.b,
            g: rgba.g,
            r: rgba.r,
            x: rgba.a,
        }
    }
    
    fn to_rgba_u8_format(self) -> crate::formats::RGBAu8 {
        crate::formats::RGBAu8{
            a: self.x,
            b: self.b,
            g: self.g,
            r: self.r,
        }
    }
    
    fn from_rgba_u8_format(rgba: crate::formats::RGBAu8) -> Self {
        Self{
            b: rgba.b,
            g: rgba.g,
            r: rgba.r,
            x: rgba.a,
        }
    }
}