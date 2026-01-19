// Not needed on all platforms
#![allow(dead_code)]

use std::cmp;
use std::fmt;
use std::num::NonZeroU32;
use std::ops;

use crate::{Pixel, Rect};

/// Calculates the smallest `Rect` necessary to represent all damaged `Rect`s.
pub(crate) fn union_damage(damage: &[Rect]) -> Option<Rect> {
    struct Region {
        left: u32,
        top: u32,
        bottom: u32,
        right: u32,
    }

    let region = damage
        .iter()
        .map(|rect| Region {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width.get(),
            bottom: rect.y + rect.height.get(),
        })
        .reduce(|mut prev, next| {
            prev.left = cmp::min(prev.left, next.left);
            prev.top = cmp::min(prev.top, next.top);
            prev.right = cmp::max(prev.right, next.right);
            prev.bottom = cmp::max(prev.bottom, next.bottom);
            prev
        })?;

    Some(Rect {
        x: region.left,
        y: region.top,
        width: NonZeroU32::new(region.right - region.left)
            .expect("`right` must always be bigger then `left`"),
        height: NonZeroU32::new(region.bottom - region.top)
            .expect("`bottom` must always be bigger then `top`"),
    })
}

/// Clamp the damage rectangle to be within the given bounds.
pub(crate) fn clamp_rect(rect: Rect, width: NonZeroU32, height: NonZeroU32) -> Rect {
    // The positions of the edges of the rectangle.
    let left = rect.x.min(width.get());
    let top = rect.y.min(height.get());
    let right = rect.x.saturating_add(rect.width.get()).min(width.get());
    let bottom = rect.y.saturating_add(rect.height.get()).min(height.get());

    Rect {
        x: left,
        y: top,
        width: NonZeroU32::new(right - left).expect("rect ended up being zero-sized"),
        height: NonZeroU32::new(bottom - top).expect("rect ended up being zero-sized"),
    }
}

/// A wrapper around a `Vec` of pixels that doesn't print the whole buffer on `Debug`.
#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct PixelBuffer(pub Vec<Pixel>);

impl fmt::Debug for PixelBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelBuffer").finish_non_exhaustive()
    }
}

impl ops::Deref for PixelBuffer {
    type Target = Vec<Pixel>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ops::DerefMut for PixelBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Convert a `u32` to `u16`, and saturate if it overflows.
pub(crate) fn to_u16_saturating(val: u32) -> u16 {
    val.try_into().unwrap_or(u16::MAX)
}

/// Convert a `u32` to `i16`, and saturate if it overflows.
pub(crate) fn to_i16_saturating(val: u32) -> i16 {
    val.try_into().unwrap_or(i16::MAX)
}

/// Convert a `u32` to `i32`, and saturate if it overflows.
pub(crate) fn to_i32_saturating(val: u32) -> i32 {
    val.try_into().unwrap_or(i32::MAX)
}

/// Compute the byte stride desired by Softbuffer when a platform can use any stride.
///
/// TODO(madsmtm): This should take the pixel format / bit depth as input after:
/// <https://github.com/rust-windowing/softbuffer/issues/98>
#[inline]
pub(crate) fn byte_stride(width: u32) -> u32 {
    let row_alignment = if cfg!(debug_assertions) {
        16 // Use a higher alignment to help users catch issues with their stride calculations.
    } else {
        4 // At least 4 is necessary for `Buffer` to return `&mut [u32]`.
    };
    // TODO: Use `next_multiple_of` when in MSRV.
    let mask = row_alignment * 4 - 1;
    ((width * 32 + mask) & !mask) >> 3
}
