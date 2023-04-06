//! Implementation of software buffering for web targets.

#![allow(clippy::uninlined_format_args)]

use raw_window_handle::WebWindowHandle;
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::CanvasRenderingContext2d;
use web_sys::HtmlCanvasElement;
use web_sys::ImageData;

use crate::SoftBufferError;
use std::convert::TryInto;
use std::num::NonZeroU32;

/// Display implementation for the web platform.
///
/// This just caches the document to prevent having to query it every time.
pub struct WebDisplayImpl {
    document: web_sys::Document,
}

impl WebDisplayImpl {
    pub(super) fn new() -> Result<Self, SoftBufferError> {
        let document = web_sys::window()
            .ok_or_else(|| {
                SoftBufferError::PlatformError(
                    Some("`window` is not present in this runtime".into()),
                    None,
                )
            })?
            .document()
            .ok_or_else(|| {
                SoftBufferError::PlatformError(
                    Some("`document` is not present in this runtime".into()),
                    None,
                )
            })?;

        Ok(Self { document })
    }
}

pub struct WebImpl {
    /// The handle to the canvas that we're drawing to.
    canvas: HtmlCanvasElement,

    /// The 2D rendering context for the canvas.
    ctx: CanvasRenderingContext2d,

    /// The buffer that we're drawing to.
    buffer: Vec<u32>,

    /// The current width of the canvas.
    width: u32,
}

impl WebImpl {
    pub fn new(display: &WebDisplayImpl, handle: WebWindowHandle) -> Result<Self, SoftBufferError> {
        let canvas: HtmlCanvasElement = display
            .document
            .query_selector(&format!("canvas[data-raw-handle=\"{}\"]", handle.id))
            // `querySelector` only throws an error if the selector is invalid.
            .unwrap()
            .ok_or_else(|| {
                SoftBufferError::PlatformError(
                    Some("No canvas found with the given id".into()),
                    None,
                )
            })?
            // We already made sure this was a canvas in `querySelector`.
            .unchecked_into();

        let ctx = canvas
        .get_context("2d")
        .map_err(|_| {
            SoftBufferError::PlatformError(
                Some("Canvas already controlled using `OffscreenCanvas`".into()),
                None,
            )
        })?
        .ok_or_else(|| {
            SoftBufferError::PlatformError(
                Some("A canvas context other than `CanvasRenderingContext2d` was already created".into()),
                None,
            )
        })?
        .dyn_into()
        .expect("`getContext(\"2d\") didn't return a `CanvasRenderingContext2d`");

        Ok(Self {
            canvas,
            ctx,
            buffer: Vec::new(),
            width: 0,
        })
    }

    /// Resize the canvas to the given dimensions.
    pub(crate) fn resize(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), SoftBufferError> {
        let width = width.get();
        let height = height.get();

        self.buffer.resize(total_len(width, height), 0);
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.width = width;
        Ok(())
    }

    /// Get a pointer to the mutable buffer.
    pub(crate) fn buffer_mut(&mut self) -> Result<BufferImpl, SoftBufferError> {
        Ok(BufferImpl { imp: self })
    }
}

pub struct BufferImpl<'a> {
    imp: &'a mut WebImpl,
}

impl<'a> BufferImpl<'a> {
    pub fn pixels(&self) -> &[u32] {
        &self.imp.buffer
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.imp.buffer
    }

    /// Push the buffer to the canvas.
    pub fn present(self) -> Result<(), SoftBufferError> {
        // Create a bitmap from the buffer.
        let bitmap: Vec<_> = self
            .imp
            .buffer
            .iter()
            .copied()
            .flat_map(|pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 255])
            .collect();

        // This should only throw an error if the buffer we pass's size is incorrect.
        let image_data =
            ImageData::new_with_u8_clamped_array(Clamped(&bitmap), self.imp.width).unwrap();

        // This can only throw an error if `data` is detached, which is impossible.
        self.imp.ctx.put_image_data(&image_data, 0.0, 0.0).unwrap();

        Ok(())
    }
}

#[inline(always)]
fn total_len(width: u32, height: u32) -> usize {
    // Convert width and height to `usize`, then multiply.
    width
        .try_into()
        .ok()
        .and_then(|w: usize| height.try_into().ok().and_then(|h| w.checked_mul(h)))
        .unwrap_or_else(|| {
            panic!(
                "Overflow when calculating total length of buffer: {}x{}",
                width, height
            );
        })
}
