// Not needed on all platforms
#![allow(dead_code)]

use std::cmp;
use std::fmt;
use std::num::NonZeroU32;
use std::ops;

use crate::Rect;

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

/// A wrapper around a `Vec` of pixels that doesn't print the whole buffer on `Debug`.
#[derive(PartialEq, Eq, Hash, Clone)]
pub(crate) struct PixelBuffer(pub Vec<u32>);

impl fmt::Debug for PixelBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelBuffer").finish_non_exhaustive()
    }
}

impl ops::Deref for PixelBuffer {
    type Target = Vec<u32>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ops::DerefMut for PixelBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
