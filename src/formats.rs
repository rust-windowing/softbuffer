/// Used for converting to and from the ```softbuffer::RGBX``` or ```softbuffer::RGBA``` 
/// in their platform specific formats, to a specific format when needed
/// 
/// Keep in mind that platform endianness still maters when creating these format values
/// as they are backed by a u32 where endianness matters. A workaround for this is to use the u32::from_be_bytes as shown in the example.
/// 
/// If wanting a completely non endian format, use one of the u8 based formats.
/// 
/// # Example:
/// ```rust
/// let red = 255;
/// let green = 0;
/// let blue = 255;
/// let alpha = 255;
/// let purple_u32_rgba = u32::from_be_bytes([red, green, blue, alpha]); //ensures is platform independent
/// RGBA::from_rgba_format(softbuffer::formats::RGBA::new_from_u32(purple_u32_rgba));
/// ```
pub trait RGBFormat{
    fn to_rgba_format(self) -> crate::formats::RGBA;
    fn from_rgba_format(rgba: crate::formats::RGBA) -> Self;
    fn to_rgba_u8_format(self) -> crate::formats::RGBAu8;
    fn from_rgba_u8_format(rgba: crate::formats::RGBAu8) -> Self;
    fn to_argb_format(self) -> crate::formats::ARGB;
    fn from_argb_format(rgba: crate::formats::ARGB) -> Self;
}

//When wanting the bytes in a specific order by u8, you no longer care about endianness
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RGBAu8{
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RGBAu8{
    /// Creates a ```softbuffer::formats::RGBAu8``` from a ```[u8;4]```.
    /// 
    /// To get a useable softbuffer::RGBA import the ```softbuffer::formats::RGBFormat``` trait, and then you can call
    /// softbuffer::RGBA::from_rgba_u8_format()
    pub fn new_from_u8_array(slice: [u8;4])->Self{
        unsafe{
            std::mem::transmute(slice)
        }
    }

    /// Converts a ```softbuffer::formats::RGBAu8``` into a ```[u8;4]```
    pub fn as_u8_array(self) -> [u8;4]{
        unsafe{
            std::mem::transmute(self)
        }
    }
}


#[cfg(target_endian = "little")]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RGBA{
    pub a: u8,
    pub b: u8,
    pub g: u8,
    pub r: u8,
}

#[cfg(target_endian = "big")]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RGBA{
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RGBA {
    /// Creates a ```softbuffer::formats::RGBA``` from a u32.
    /// 
    /// To get a useable softbuffer::RGBA import the ```softbuffer::formats::RGBFormat``` trait, and then you can call
    /// softbuffer::RGBA::from_rgba_format()
    pub fn new_from_u32(u32: u32)->Self{
        unsafe{
            std::mem::transmute(u32)
        }
    }

    /// Converts a ```softbuffer::formats::RGBA``` into a u32
    pub fn as_u32(self) -> u32{
        unsafe{
            std::mem::transmute(self)
        }
    }
}

#[cfg(target_endian = "little")]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ARGB{
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

#[cfg(target_endian = "big")]
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ARGB{
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ARGB {
    /// Creates a ```softbuffer::formats::ARGB``` from a u32.
    /// 
    /// To get a useable softbuffer::RGBA import the ```softbuffer::formats::RGBFormat``` trait, and then you can call
    /// softbuffer::RGBA::from_argb_format()
    pub fn new_from_u32(u32: u32)->Self{
        unsafe{
            std::mem::transmute(u32)
        }
    }

    /// Converts a ```softbuffer::formats::ARGB``` into a u32
    pub fn as_u32(self) -> u32{
        unsafe{
            std::mem::transmute(self)
        }
    }
}