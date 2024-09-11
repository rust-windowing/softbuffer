pub trait RGBFormat{
    fn to_rgba_format(self) -> crate::formats::RGBA;
    fn from_rgba_format(rgba: crate::formats::RGBA) -> Self;
    fn to_rgba_u8_format(self) -> crate::formats::RGBAu8;
    fn from_rgba_u8_format(rgba: crate::formats::RGBAu8) -> Self;
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