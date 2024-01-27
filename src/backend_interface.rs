//! Interface implemented by backends

use crate::{Rect, SoftBufferError};

use raw_window_handle::HasWindowHandle;
use std::num::NonZeroU32;

pub(crate) trait SurfaceInterface<W: HasWindowHandle + ?Sized> {
    type Buffer<'a>: BufferInterface
    where
        Self: 'a;

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

pub(crate) trait BufferInterface {
    fn pixels(&self) -> &[u32];
    fn pixels_mut(&mut self) -> &mut [u32];
    fn age(&self) -> u8;
    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError>;
    fn present(self) -> Result<(), SoftBufferError>;
}
