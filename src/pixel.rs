/// A pixel.
///
/// # Representation
///
/// This is a set of `u8`'s in the order BGRX (first component blue, second green, third red and
/// last unset), except on WebAssembly and Android targets, there it is RGBX, since the API on these
/// platforms only support that format. This distinction should only be relevant if you're
/// bitcasting `Pixel` to/from a `u32`.
///
/// If you're familiar with [the `rgb` crate](https://docs.rs/rgb/), you can treat this mostly as-if
/// it was defined as follows:
///
/// ```ignore
/// #[cfg(any(target_family = "wasm", target_os = "android"))]
/// type Pixel = rgb::Rgba<u8>;
/// #[cfg(not(any(target_family = "wasm", target_os = "android")))]
/// type Pixel = rgb::Bgra<u8>;
/// ```
///
/// # Example
///
/// Construct a new pixel.
///
/// ```
/// # use softbuffer::Pixel;
/// #
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
/// # use softbuffer::Pixel;
/// #
/// let red = Pixel::new_rgb(0xff, 0, 0);
/// // SAFETY: `Pixel` can be reinterpreted as `[u8; 4]`.
/// let red = unsafe { core::mem::transmute::<Pixel, [u8; 4]>(red) };
///
/// if cfg!(any(target_family = "wasm", target_os = "android")) {
///     // RGBX
///     assert_eq!(red[0], 255);
/// } else {
///     // BGRX
///     assert_eq!(red[2], 255);
/// }
/// ```
///
/// Convert a pixel to an `u32`.
///
/// ```
/// # use softbuffer::Pixel;
/// #
/// let red = Pixel::new_rgb(0xff, 0, 0);
/// // SAFETY: `Pixel` can be reinterpreted as `u32`.
/// let red = unsafe { core::mem::transmute::<Pixel, u32>(red) };
///
/// if cfg!(any(target_family = "wasm", target_os = "android")) {
///     // RGBX
///     assert_eq!(red, u32::from_ne_bytes([0xff, 0x00, 0x00, 0xff]));
/// } else {
///     // BGRX
///     assert_eq!(red, u32::from_ne_bytes([0x00, 0x00, 0xff, 0xff]));
/// }
/// ```
#[repr(C)]
#[repr(align(4))] // May help the compiler to see that this is a u32
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pixel {
    #[cfg(any(target_family = "wasm", target_os = "android"))]
    /// The red component.
    pub r: u8,
    #[cfg(any(target_family = "wasm", target_os = "android"))]
    /// The green component.
    pub g: u8,
    #[cfg(any(target_family = "wasm", target_os = "android"))]
    /// The blue component.
    pub b: u8,

    #[cfg(not(any(target_family = "wasm", target_os = "android")))]
    /// The blue component.
    pub b: u8,
    #[cfg(not(any(target_family = "wasm", target_os = "android")))]
    /// The green component.
    pub g: u8,
    #[cfg(not(any(target_family = "wasm", target_os = "android")))]
    /// The red component.
    pub r: u8,

    /// The alpha component.
    ///
    /// `0xff` here means opaque, whereas `0` means transparent.
    ///
    /// NOTE: Transparency is yet poorly supported, see [#17], until that is resolved, you will
    /// probably want to set this to `0xff`.
    ///
    /// [#17]: https://github.com/rust-windowing/softbuffer/issues/17
    pub a: u8,
}

impl Pixel {
    /// Creates a new pixel from a red, a green and a blue component.
    ///
    /// The alpha component is set to opaque.
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

    /// Creates a new pixel from a blue, a green and a red component.
    ///
    /// The alpha component is set to opaque.
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

    // TODO: Once we have transparency, add `new_rgba` and `new_bgra` methods.
}

// TODO: Implement `Add`/`Mul`/similar `std::ops` like `rgb` does?

// TODO: Implement `zerocopy` traits behind a feature flag?
// May not be that useful, since the representation is platform-specific.
