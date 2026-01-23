#[inline]
pub(crate) fn premultiply(val: u8, alpha: u8) -> u8 {
    // TODO: Do we need to optimize this using bit shifts or similar?
    ((val as u16 * alpha as u16) / 0xff) as u8
}

#[inline]
pub(crate) fn unpremultiply(val: u8, alpha: u8) -> u8 {
    // TODO: Can we find a cleaner / more efficient way to implement this?
    (val as u16 * u8::MAX as u16)
        .checked_div(alpha as u16)
        .unwrap_or(0)
        .min(u8::MAX as u16) as u8
}
