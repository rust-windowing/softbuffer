//! Implementation of software buffering for OpenHarmony.

use std::marker::PhantomData;
use std::num::{NonZeroI32, NonZeroU32};

#[cfg(doc)]
use raw_window_handle::OhosNdkWindowHandle;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

use crate::error::InitError;
use crate::{BufferInterface, Rect, SoftBufferError, SurfaceInterface};
use ohos_native_window_binding::{NativeBufferFormat, NativeWindow, NativeWindowBuffer};

/// The handle to a window for software buffering.
pub struct OpenHarmonyImpl<D, W> {
    native_window: NativeWindow,
    window: W,
    _display: PhantomData<D>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for OpenHarmonyImpl<D, W> {
    type Context = D;
    type Buffer<'a>
        = BufferImpl<'a, D, W>
    where
        Self: 'a;

    /// Create a new [`OpenHarmonyImpl`] from an [`OhosNdkWindowHandle`].
    fn new(window: W, _display: &Self::Context) -> Result<Self, InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::OhosNdk(a) = raw else {
            return Err(InitError::Unsupported(window));
        };

        // Acquire a new owned reference to the window, that will be freed on drop.
        // SAFETY: We have confirmed that the window handle is valid.
        let native_window = NativeWindow::clone_from_ptr(a.native_window.as_ptr());

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
            .set_buffer_geometry(width.into(), height.into())
            .map_err(|err| {
                SoftBufferError::PlatformError(
                    Some("Failed to set buffer geometry on NativeWindow".to_owned()),
                    Some(Box::new(err)),
                )
            })
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let native_window_buffer = self.native_window.request_buffer(None).map_err(|err| {
            SoftBufferError::PlatformError(
                Some("Failed to request native window buffer".to_owned()),
                Some(Box::new(err)),
            )
        })?;

        if !matches!(
            native_window_buffer.format(),
            // These are the only formats we support
            NativeBufferFormat::RGBA_8888 | NativeBufferFormat::RGBX_8888
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
        let size = (native_window_buffer.width() * native_window_buffer.height())
            .try_into()
            .map_err(|e| {
                SoftBufferError::PlatformError(
                    Some("Failed to convert width to u32".to_owned()),
                    Some(Box::new(e)),
                )
            })?;
        let buffer = vec![0; size];

        Ok(BufferImpl {
            native_window_buffer,
            buffer,
            marker: PhantomData,
        })
    }

    /// Fetch the buffer from the window.
    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }
}

pub struct BufferImpl<'a, D: ?Sized, W> {
    native_window_buffer: NativeWindowBuffer<'a>,
    buffer: Vec<u32>,
    marker: PhantomData<(&'a D, &'a W)>,
}

unsafe impl<'a, D, W> Send for BufferImpl<'a, D, W> {}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'a, D, W> {
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
        let input_lines = self.buffer.chunks(self.native_window_buffer.width() as _);
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
                // Swizzle colors from RGBX to BGR
                let [b, g, r, _] = pixel.to_le_bytes();
                output[i * 4].write(b);
                output[i * 4 + 1].write(g);
                output[i * 4 + 2].write(r);
            }
        }
        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.present()
    }
}
