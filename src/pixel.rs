/// A 32-bit pixel with 4 components.
///
/// # Representation
///
/// This is a set of 4 `u8`'s laid out in the order defined by [`PixelFormat::default()`].
///
/// This type has an alignment of `4` as that makes copies faster on many platforms, and makes this
/// type have the same in-memory representation as a `u32`.
///
/// [`PixelFormat::default()`]: crate::PixelFormat#default
///
/// # Default
///
/// The [`Default`] impl returns a transparent black pixel. Beware that this might not be what you
/// want if using [`AlphaMode::Opaque`] (which is the default).
///
/// [`AlphaMode::Opaque`]: crate::AlphaMode::Opaque
///
/// # Example
///
/// Construct a new pixel.
///
/// ```
/// use softbuffer::Pixel;
///
/// let red = Pixel::new_rgb(0xff, 0x80, 0);
/// assert_eq!(red.r, 255);
/// assert_eq!(red.g, 128);
/// assert_eq!(red.b, 0);
/// assert_eq!(red.a, 0xff);
///
/// let from_struct_literal = Pixel { r: 255, g: 0x80, b: 0, a: 0xff };
/// assert_eq!(red, from_struct_literal);
/// ```
///
/// Convert a pixel to an array of `u8`s.
///
/// ```
/// use softbuffer::{Pixel, PixelFormat};
///
/// let red = Pixel::new_rgb(0xff, 0, 0);
/// // SAFETY: `Pixel` can be reinterpreted as `[u8; 4]`.
/// let red = unsafe { core::mem::transmute::<Pixel, [u8; 4]>(red) };
///
/// match PixelFormat::default() {
///     PixelFormat::Bgra => assert_eq!(red[2], 255),
///     PixelFormat::Rgba => assert_eq!(red[0], 255),
/// }
/// ```
///
/// Convert a pixel to a `u32`.
///
/// ```
/// use softbuffer::{Pixel, PixelFormat};
///
/// let red = Pixel::new_rgb(0xff, 0, 0);
/// // SAFETY: `Pixel` can be reinterpreted as `u32`.
/// let red = unsafe { core::mem::transmute::<Pixel, u32>(red) };
///
/// match PixelFormat::default() {
///     PixelFormat::Bgra => assert_eq!(red, u32::from_ne_bytes([0x00, 0x00, 0xff, 0xff])),
///     PixelFormat::Rgba => assert_eq!(red, u32::from_ne_bytes([0xff, 0x00, 0x00, 0xff])),
/// }
/// ```
#[repr(C)]
#[repr(align(4))] // Help the compiler to see that this is a u32
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Pixel {
    #[cfg_attr(docsrs, doc(auto_cfg = false))]
    #[cfg(any(doc, target_family = "wasm", target_os = "android"))]
    /// The red component.
    pub r: u8,
    #[cfg(not(any(doc, target_family = "wasm", target_os = "android")))]
    /// The blue component.
    pub b: u8,

    /// The green component.
    pub g: u8,

    #[cfg_attr(docsrs, doc(auto_cfg = false))]
    #[cfg(any(doc, target_family = "wasm", target_os = "android"))]
    /// The blue component.
    pub b: u8,
    #[cfg(not(any(doc, target_family = "wasm", target_os = "android")))]
    /// The red component.
    pub r: u8,

    /// The alpha component.
    ///
    /// `0xff` here means opaque, whereas `0` means transparent.
    ///
    /// Make sure to set this correctly according to the [`AlphaMode`][crate::AlphaMode].
    pub a: u8,
}

impl Pixel {
    /// Create a new opaque pixel from a red, a green and a blue component.
    ///
    /// # Example
    ///
    /// ```
    /// # use softbuffer::Pixel;
    /// #
    /// let red = Pixel::new_rgb(0xff, 0, 0);
    /// assert_eq!(red.r, 255);
    /// ```
    pub const fn new_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Create a new opaque pixel from a blue, a green and a red component.
    ///
    /// # Example
    ///
    /// ```
    /// # use softbuffer::Pixel;
    /// #
    /// let red = Pixel::new_bgr(0, 0, 0xff);
    /// assert_eq!(red.r, 255);
    /// ```
    pub const fn new_bgr(b: u8, g: u8, r: u8) -> Self {
        Self { r, g, b, a: 0xff }
    }

    /// Create a new pixel from a red, a green, a blue and an alpha component.
    ///
    /// # Example
    ///
    /// ```
    /// # use softbuffer::Pixel;
    /// #
    /// let red = Pixel::new_rgba(0xff, 0, 0, 0x7f);
    /// assert_eq!(red.r, 255);
    /// assert_eq!(red.a, 127);
    /// ```
    pub const fn new_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create a new pixel from a blue, a green, a red and an alpha component.
    ///
    /// # Example
    ///
    /// ```
    /// # use softbuffer::Pixel;
    /// #
    /// let red = Pixel::new_bgra(0, 0, 0xff, 0x7f);
    /// assert_eq!(red.r, 255);
    /// assert_eq!(red.a, 127);
    /// ```
    pub const fn new_bgra(b: u8, g: u8, r: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

// TODO: Implement `Add`/`Mul`/similar `std::ops` like `rgb` does?

// TODO: Implement `zerocopy` / `bytemuck` traits behind a feature flag?
// May not be that useful, since the representation is platform-specific.
