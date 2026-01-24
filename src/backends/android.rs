//! Implementation of software buffering for Android.

use std::marker::PhantomData;
use std::num::{NonZeroI32, NonZeroU32};

use ndk::{
    hardware_buffer_format::HardwareBufferFormat,
    native_window::{NativeWindow, NativeWindowBufferLockGuard},
};
#[cfg(doc)]
use raw_window_handle::AndroidNdkWindowHandle;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use crate::error::InitError;
use crate::{util, BufferInterface, Pixel, Rect, SoftBufferError, SurfaceInterface};

/// The handle to a window for software buffering.
#[derive(Debug)]
pub struct AndroidImpl<D, W> {
    native_window: NativeWindow,
    window: W,
    _display: PhantomData<D>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for AndroidImpl<D, W> {
    type Context = D;
    type Buffer<'a>
        = BufferImpl<'a>
    where
        Self: 'a;

    /// Create a new [`AndroidImpl`] from an [`AndroidNdkWindowHandle`].
    fn new(window: W, _display: &Self::Context) -> Result<Self, InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::AndroidNdk(a) = raw else {
            return Err(InitError::Unsupported(window));
        };

        // Acquire a new owned reference to the window, that will be freed on drop.
        // SAFETY: We have confirmed that the window handle is valid.
        let native_window = unsafe { NativeWindow::clone_from_ptr(a.a_native_window.cast()) };

        Ok(Self {
            native_window,
            _display: PhantomData,
            window,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window
    }

    /// Also changes the pixel format to [`HardwareBufferFormat::R8G8B8A8_UNORM`].
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let (width, height) = (|| {
            let width = NonZeroI32::try_from(width).ok()?;
            let height = NonZeroI32::try_from(height).ok()?;
            Some((width, height))
        })()
        .ok_or(SoftBufferError::SizeOutOfRange { width, height })?;

        self.native_window
            .set_buffers_geometry(
                width.into(),
                height.into(),
                // Default is typically R5G6B5 16bpp, switch to 32bpp
                Some(HardwareBufferFormat::R8G8B8X8_UNORM),
            )
            .map_err(|err| {
                SoftBufferError::PlatformError(
                    Some("Failed to set buffer geometry on ANativeWindow".to_owned()),
                    Some(Box::new(err)),
                )
            })
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_>, SoftBufferError> {
        let native_window_buffer = self.native_window.lock(None).map_err(|err| {
            SoftBufferError::PlatformError(
                Some("Failed to lock ANativeWindow".to_owned()),
                Some(Box::new(err)),
            )
        })?;

        if !matches!(
            native_window_buffer.format(),
            // These are the only formats we support
            HardwareBufferFormat::R8G8B8A8_UNORM | HardwareBufferFormat::R8G8B8X8_UNORM
        ) {
            return Err(SoftBufferError::PlatformError(
                Some(format!(
                    "Unexpected buffer format {:?}, please call \
                    .resize() first to change it to RGBx8888",
                    native_window_buffer.format()
                )),
                None,
            ));
        }

        let buffer =
            vec![Pixel::default(); native_window_buffer.width() * native_window_buffer.height()];

        Ok(BufferImpl {
            native_window_buffer,
            buffer: util::PixelBuffer(buffer),
        })
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<Pixel>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

#[derive(Debug)]
pub struct BufferImpl<'a> {
    native_window_buffer: NativeWindowBufferLockGuard<'a>,
    buffer: util::PixelBuffer,
}

// TODO: Move to NativeWindowBufferLockGuard?
unsafe impl Send for BufferImpl<'_> {}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width().get() * 4).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.native_window_buffer.width() as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.native_window_buffer.height() as u32).unwrap()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        &mut self.buffer
    }

    #[inline]
    fn age(&self) -> u8 {
        0
    }

    // TODO: This function is pretty slow this way
    fn present_with_damage(mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        // TODO: Android requires the damage rect _at lock time_
        // Since we're faking the backing buffer _anyway_, we could even fake the surface lock
        // and lock it here (if it doesn't influence timings).
        //
        // Android seems to do this because the region can be expanded by the
        // system, requesting the user to actually redraw a larger region.
        // It's unclear if/when this is used, or if corruption/artifacts occur
        // when the enlarged damage region is not re-rendered?
        let _ = damage;

        let input_lines = self.buffer.chunks(self.native_window_buffer.width());
        for (output, input) in self
            .native_window_buffer
            .lines()
            // Unreachable as we checked before that this is a valid, mappable format
            .unwrap()
            .zip(input_lines)
        {
            // .lines() removed the stride
            assert_eq!(output.len(), input.len() * 4);

            // Write RGB(A) to the output.
            // TODO: Use `slice::write_copy_of_slice` once stable and in MSRV.
            // TODO(madsmtm): Verify that this compiles down to an efficient copy.
            for (i, pixel) in input.iter().enumerate() {
                output[i * 4].write(pixel.r);
                output[i * 4 + 1].write(pixel.g);
                output[i * 4 + 2].write(pixel.b);
                output[i * 4 + 3].write(pixel.a);
            }
        }
        Ok(())
    }
}
