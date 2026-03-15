// Not needed on all platforms
#![allow(dead_code)]

use std::cmp;
use std::fmt;
use std::num::NonZeroU32;
use std::ops;

use crate::{Pixel, Rect};

/// The positions at the edge of a rectangle.
#[derive(Default)]
struct Region {
    left: u32,
    top: u32,
    // Invariant: left <= right
    right: u32,
    // Invariant: top <= bottom
    bottom: u32,
}

impl Region {
    fn from_rect(rect: Rect) -> Self {
        Self {
            left: rect.x,
            top: rect.y,
            right: rect.x.saturating_add(rect.width),
            bottom: rect.y.saturating_add(rect.height),
        }
    }

    fn into_rect(self) -> Rect {
        Rect {
            x: self.left,
            y: self.top,
            width: self.right - self.left,
            height: self.bottom - self.top,
        }
    }
}

/// Calculates the smallest `Rect` necessary to represent all damaged `Rect`s.
pub(crate) fn union_damage(damage: &[Rect]) -> Rect {
    damage
        .iter()
        .map(|rect| Region::from_rect(*rect))
        .reduce(|mut prev, next| {
            prev.left = cmp::min(prev.left, next.left);
            prev.top = cmp::min(prev.top, next.top);
            prev.right = cmp::max(prev.right, next.right);
            prev.bottom = cmp::max(prev.bottom, next.bottom);
            prev
        })
        .unwrap_or_default()
        .into_rect()
}

/// Clamp the damage rectangle to be within the given bounds.
pub(crate) fn clamp_rect(rect: Rect, width: NonZeroU32, height: NonZeroU32) -> Rect {
    let mut region = Region::from_rect(rect);

    region.left = region.left.min(width.get());
    region.top = region.top.min(height.get());
    region.right = region.right.min(width.get());
    region.bottom = region.bottom.min(height.get());

    region.into_rect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union() {
        // Zero-sized.
        let res = union_damage(&[]);
        assert_eq!(res.x, 0);
        assert_eq!(res.y, 0);
        assert_eq!(res.width, 0);
        assert_eq!(res.height, 0);

        let res = union_damage(&[
            Rect {
                x: 100,
                y: 20,
                width: 30,
                height: 40,
            },
            Rect {
                x: 10,
                y: 200,
                width: 30,
                height: 40,
            },
        ]);
        assert_eq!(res.x, 10);
        assert_eq!(res.y, 20);
        assert_eq!(res.width, 120);
        assert_eq!(res.height, 220);
    }

    // This is not a guarantee either way, just a test of the current implementation.
    #[test]
    fn union_considers_zero_sized() {
        let res = union_damage(&[
            Rect {
                x: 100,
                y: 20,
                width: 0,
                height: 40,
            },
            Rect {
                x: 10,
                y: 200,
                width: 30,
                height: 0,
            },
        ]);
        assert_eq!(res.x, 10);
        assert_eq!(res.y, 20);
        assert_eq!(res.width, 90);
        assert_eq!(res.height, 180);
    }

    #[test]
    fn clamp() {
        let rect = Rect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };

        // Inside bounds
        let res = clamp_rect(
            rect,
            NonZeroU32::new(50).unwrap(),
            NonZeroU32::new(60).unwrap(),
        );
        assert_eq!(res.x, 10);
        assert_eq!(res.y, 20);
        assert_eq!(res.width, 30);
        assert_eq!(res.height, 40);

        // Size out of bounds
        let res = clamp_rect(
            rect,
            NonZeroU32::new(33).unwrap(),
            NonZeroU32::new(44).unwrap(),
        );
        assert_eq!(res.x, 10);
        assert_eq!(res.y, 20);
        assert_eq!(res.width, 23);
        assert_eq!(res.height, 24);

        // Fully beyond bounds
        let res = clamp_rect(
            rect,
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(2).unwrap(),
        );
        assert_eq!(res.x, 1);
        assert_eq!(res.y, 2);
        assert_eq!(res.width, 0);
        assert_eq!(res.height, 0);
    }
}
