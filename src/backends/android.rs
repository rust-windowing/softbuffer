//! Implementation of software buffering for Android.

use std::marker::PhantomData;
use std::num::{NonZeroI32, NonZeroU32};

use ndk::{
    hardware_buffer_format::HardwareBufferFormat,
    native_window::{NativeWindow, NativeWindowBufferLockGuard},
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use crate::error::InitError;
use crate::{Rect, SoftBufferError};

/// The handle to a window for software buffering.
pub struct AndroidImpl<D: ?Sized, W: ?Sized> {
    native_window: NativeWindow,

    _display: PhantomData<D>,

    /// The pointer to the window object.
    ///
    /// This is pretty useless because it gives us a pointer to [`NativeWindow`] that we have to increase the refcount on.
    /// Alternatively we can use [`NativeWindow::from_ptr()`] wrapped in [`std::mem::ManuallyDrop`]
    window: W,
}

// TODO: Current system doesn't require a trait to be implemented here, even though it exists.
impl<D: HasDisplayHandle, W: HasWindowHandle> AndroidImpl<D, W> {
    /// Create a new [`AndroidImpl`] from an [`AndroidNdkWindowHandle`].
    ///
    /// # Safety
    ///
    /// The [`AndroidNdkWindowHandle`] must be a valid window handle.
    // TODO: That's lame, why can't we get an AndroidNdkWindowHandle directly here
    pub(crate) fn new(window: W, _display: &D) -> Result<Self, InitError<W>> {
        // Get the raw Android window (surface).
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::AndroidNdk(a) = raw else {
            return Err(InitError::Unsupported(window));
        };

        // Acquire a new owned reference to the window, that will be freed on drop.
        let native_window = unsafe { NativeWindow::clone_from_ptr(a.a_native_window.cast()) };

        Ok(Self {
            native_window,
            // _display: DisplayHandle::borrow_raw(raw_window_handle::RawDisplayHandle::Android(
            //     AndroidDisplayHandle,
            // )),
            _display: PhantomData,
            window,
        })
    }

    #[inline]
    pub fn window(&self) -> &W {
        &self.window
    }

    /// Also changes the pixel format to [`HardwareBufferFormat::R8G8B8A8_UNORM`].
    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let (width, height) = (|| {
            let width = NonZeroI32::try_from(width).ok()?;
            let height = NonZeroI32::try_from(height).ok()?;
            Some((width, height))
        })()
        .ok_or(SoftBufferError::SizeOutOfRange { width, height })?;

        // Do not change the format.
        self.native_window
            .set_buffers_geometry(
                width.into(),
                height.into(),
                // Default is typically R5G6B5 16bpp, switch to 32bpp
                Some(HardwareBufferFormat::R8G8B8A8_UNORM),
            )
            .map_err(|err| {
                SoftBufferError::PlatformError(
                    Some("Failed to set buffer geometry on ANativeWindow".to_owned()),
                    Some(Box::new(err)),
                )
            })
    }

    pub fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let lock_guard = self.native_window.lock(None).map_err(|err| {
            SoftBufferError::PlatformError(
                Some("Failed to lock ANativeWindow".to_owned()),
                Some(Box::new(err)),
            )
        })?;

        assert_eq!(
            lock_guard.format().bytes_per_pixel(),
            Some(4),
            "Unexpected buffer format {:?}, please call .resize() first to change it to RGBA8888",
            lock_guard.format()
        );

        Ok(BufferImpl(lock_guard, PhantomData, PhantomData))
    }

    /// Fetch the buffer from the window.
    pub fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

pub struct BufferImpl<'a, D: ?Sized, W>(
    NativeWindowBufferLockGuard<'a>,
    PhantomData<&'a D>,
    PhantomData<&'a W>,
);

// TODO: Move to NativeWindowBufferLockGuard?
unsafe impl<'a, D, W> Send for BufferImpl<'a, D, W> {}

impl<'a, D: HasDisplayHandle + ?Sized, W: HasWindowHandle> BufferImpl<'a, D, W> {
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        todo!()
        // unsafe {
        //     std::slice::from_raw_parts(
        //         self.0.bits().cast_const().cast(),
        //         (self.0.stride() * self.0.height()) as usize,
        //     )
        // }
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        let bytes = self.0.bytes().expect("Nonplanar format");
        unsafe {
            std::slice::from_raw_parts_mut(
                bytes.as_mut_ptr().cast(),
                bytes.len() / std::mem::size_of::<u32>(),
            )
        }
    }

    pub fn age(&self) -> u8 {
        0
    }

    pub fn present(self) -> Result<(), SoftBufferError> {
        // Dropping the guard automatically unlocks and posts it
        Ok(())
    }

    pub fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}
