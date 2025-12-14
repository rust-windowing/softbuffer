//! Implementation of software buffering for Android.

use std::marker::PhantomData;
use std::mem::MaybeUninit;

use ndk::{
    hardware_buffer_format::HardwareBufferFormat,
    native_window::{NativeWindow, NativeWindowBufferLockGuard},
};
#[cfg(doc)]
use raw_window_handle::AndroidNdkWindowHandle;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use crate::error::InitError;
use crate::{util, BufferInterface, Rect, SoftBufferError, SurfaceInterface};

/// The handle to a window for software buffering.
#[derive(Debug)]
pub struct AndroidImpl<D, W> {
    native_window: NativeWindow,
    width: u32,
    height: u32,
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
            width: 0,
            height: 0,
            _display: PhantomData,
            window,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window
    }

    /// Also changes the pixel format to [`HardwareBufferFormat::R8G8B8A8_UNORM`].
    fn resize(&mut self, width: u32, height: u32) -> Result<(), SoftBufferError> {
        let (width_i32, height_i32) = util::convert_size::<i32>(width, height)
            .map_err(|_| SoftBufferError::SizeOutOfRange { width, height })?;

        // Make the Window's buffer be at least 1 pixel wide/high.
        self.native_window
            .set_buffers_geometry(
                width_i32.max(1),
                height_i32.max(1),
                // Default is typically R5G6B5 16bpp, switch to 32bpp
                Some(HardwareBufferFormat::R8G8B8X8_UNORM),
            )
            .map_err(|err| {
                SoftBufferError::PlatformError(
                    Some("Failed to set buffer geometry on ANativeWindow".to_owned()),
                    Some(Box::new(err)),
                )
            })?;
        self.width = width;
        self.height = height;

        Ok(())
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

        let buffer = vec![0; self.width as usize * self.height as usize];

        Ok(BufferImpl {
            native_window_buffer,
            width: self.width,
            height: self.height,
            buffer: util::PixelBuffer(buffer),
        })
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

#[derive(Debug)]
pub struct BufferImpl<'a> {
    native_window_buffer: NativeWindowBufferLockGuard<'a>,
    width: u32,
    height: u32,
    buffer: util::PixelBuffer,
}

// TODO: Move to NativeWindowBufferLockGuard?
unsafe impl Send for BufferImpl<'_> {}

impl BufferInterface for BufferImpl<'_> {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    fn pixels(&self) -> &[u32] {
        &self.buffer
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }

    #[inline]
    fn age(&self) -> u8 {
        0
    }

    // TODO: This function is pretty slow this way
    fn present(mut self) -> Result<(), SoftBufferError> {
        if self.width == 0 || self.height == 0 {
            for line in self.native_window_buffer.lines().unwrap() {
                line.fill(MaybeUninit::new(0x00000000));
            }
            return Ok(());
        }

        let input_lines = self.buffer.chunks(self.width as usize);
        for (output, input) in self
            .native_window_buffer
            .lines()
            // Unreachable as we checked before that this is a valid, mappable format
            .unwrap()
            .zip(input_lines)
        {
            // .lines() removed the stride
            assert_eq!(output.len(), input.len() * 4);

            for (i, pixel) in input.iter().enumerate() {
                // Swizzle colors from BGR(A) to RGB(A)
                let [b, g, r, a] = pixel.to_le_bytes();
                output[i * 4].write(r);
                output[i * 4 + 1].write(g);
                output[i * 4 + 2].write(b);
                output[i * 4 + 3].write(a);
            }
        }
        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        // TODO: Android requires the damage rect _at lock time_
        // Since we're faking the backing buffer _anyway_, we could even fake the surface lock
        // and lock it here (if it doesn't influence timings).
        self.present()
    }
}
