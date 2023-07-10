// Not needed on all platforms
#![allow(dead_code)]

use std::cmp;
use std::num::NonZeroU32;

use crate::Rect;
use crate::SoftBufferError;

/// Takes a mutable reference to a container and a function deriving a
/// reference into it, and stores both, making it possible to get back the
/// reference to the container once the other reference is no longer needed.
///
/// This should be consistent with stacked borrow rules, and miri seems to
/// accept it at least in simple cases.
pub struct BorrowStack<'a, T: 'a + ?Sized, U: 'a + ?Sized> {
    container: *mut T,
    member: *mut U,
    _phantom: std::marker::PhantomData<&'a mut T>,
}

impl<'a, T: 'a + ?Sized, U: 'a + ?Sized> BorrowStack<'a, T, U> {
    pub fn new<F>(container: &'a mut T, f: F) -> Result<Self, SoftBufferError>
    where
        F: for<'b> FnOnce(&'b mut T) -> Result<&'b mut U, SoftBufferError>,
    {
        let container = container as *mut T;
        let member = f(unsafe { &mut *container })? as *mut U;
        Ok(Self {
            container,
            member,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn member(&self) -> &U {
        unsafe { &*self.member }
    }

    pub fn member_mut(&mut self) -> &mut U {
        unsafe { &mut *self.member }
    }

    pub fn into_container(self) -> &'a mut T {
        // SAFETY: Since we consume self and no longer reference member, this
        // mutable reference is unique.
        unsafe { &mut *self.container }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_borrowstack_slice_int() {
        fn f(mut stack: BorrowStack<[u32], u32>) {
            assert_eq!(*stack.member(), 3);
            *stack.member_mut() = 42;
            assert_eq!(stack.into_container(), &[1, 2, 42, 4, 5]);
        }

        let mut v = vec![1, 2, 3, 4, 5];
        f(BorrowStack::new(v.as_mut(), |v: &mut [u32]| Ok(&mut v[2])).unwrap());
        assert_eq!(&v, &[1, 2, 42, 4, 5]);
    }
}
