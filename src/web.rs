//! Implementation of software buffering for web targets.

#![allow(clippy::uninlined_format_args)]

use raw_window_handle::WebWindowHandle;
use wasm_bindgen::JsCast;
use web_sys::CanvasRenderingContext2d;
use web_sys::HtmlCanvasElement;
use web_sys::ImageData;

use crate::error::SwResultExt;
use crate::{Rect, SoftBufferError};
use std::convert::TryInto;
use std::marker::PhantomData;
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
            .swbuf_err("`window` is not present in this runtime")?
            .document()
            .swbuf_err("`document` is not present in this runtime")?;

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

    /// Buffer has been presented.
    buffer_presented: bool,

    /// The current canvas width/height.
    size: Option<(NonZeroU32, NonZeroU32)>,
}

impl WebImpl {
    pub fn new(display: &WebDisplayImpl, handle: WebWindowHandle) -> Result<Self, SoftBufferError> {
        let canvas: HtmlCanvasElement = display
            .document
            .query_selector(&format!("canvas[data-raw-handle=\"{}\"]", handle.id))
            // `querySelector` only throws an error if the selector is invalid.
            .unwrap()
            .swbuf_err("No canvas found with the given id")?
            // We already made sure this was a canvas in `querySelector`.
            .unchecked_into();

        Self::from_canvas(canvas)
    }

    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError> {
        let ctx = canvas
            .get_context("2d")
            .ok()
            .swbuf_err("Canvas already controlled using `OffscreenCanvas`")?
            .swbuf_err(
                "A canvas context other than `CanvasRenderingContext2d` was already created",
            )?
            .dyn_into()
            .expect("`getContext(\"2d\") didn't return a `CanvasRenderingContext2d`");

        Ok(Self {
            canvas,
            ctx,
            buffer: Vec::new(),
            buffer_presented: false,
            size: None,
        })
    }

    /// Resize the canvas to the given dimensions.
    pub(crate) fn resize(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), SoftBufferError> {
        if self.size != Some((width, height)) {
            self.buffer_presented = false;
            self.buffer.resize(total_len(width.get(), height.get()), 0);
            self.canvas.set_width(width.get());
            self.canvas.set_height(height.get());
            self.size = Some((width, height));
        }

        Ok(())
    }

    /// Get a pointer to the mutable buffer.
    pub(crate) fn buffer_mut(&mut self) -> Result<BufferImpl, SoftBufferError> {
        Ok(BufferImpl { imp: self })
    }

    fn present_with_damage(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let (width, _height) = self
            .size
            .expect("Must set size of surface before calling `present_with_damage()`");
        // Create a bitmap from the buffer.
        let bitmap: Vec<_> = self
            .buffer
            .iter()
            .copied()
            .flat_map(|pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 255])
            .collect();

        #[cfg(target_feature = "atomics")]
        let result = {
            use js_sys::{Uint8Array, Uint8ClampedArray};
            use wasm_bindgen::prelude::wasm_bindgen;
            use wasm_bindgen::JsValue;

            #[wasm_bindgen]
            extern "C" {
                #[wasm_bindgen(js_name = ImageData)]
                type ImageDataExt;

                #[wasm_bindgen(catch, constructor, js_class = ImageData)]
                fn new(array: Uint8ClampedArray, sw: u32) -> Result<ImageDataExt, JsValue>;
            }

            let array = Uint8Array::new_with_length(bitmap.len() as u32);
            array.copy_from(&bitmap);
            let array = Uint8ClampedArray::new(&array);
            ImageDataExt::new(array, width.get())
                .map(JsValue::from)
                .map(ImageData::unchecked_from_js)
        };
        #[cfg(not(target_feature = "atomics"))]
        let result =
            ImageData::new_with_u8_clamped_array(wasm_bindgen::Clamped(&bitmap), width.get());
        // This should only throw an error if the buffer we pass's size is incorrect.
        let image_data = result.unwrap();

        for rect in damage {
            // This can only throw an error if `data` is detached, which is impossible.
            self.ctx
                .put_image_data_with_dirty_x_and_dirty_y_and_dirty_width_and_dirty_height(
                    &image_data,
                    rect.x.into(),
                    rect.y.into(),
                    rect.x.into(),
                    rect.y.into(),
                    rect.width.get().into(),
                    rect.height.get().into(),
                )
                .unwrap();
        }

        self.buffer_presented = true;

        Ok(())
    }
}

/// Extension methods for the Wasm target on [`Surface`](crate::Surface).
pub trait SurfaceExtWeb: Sized {
    /// Creates a new instance of this struct, using the provided [`HtmlCanvasElement`].
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError>;
}

impl SurfaceExtWeb for crate::Surface {
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError> {
        let imple = crate::SurfaceDispatch::Web(WebImpl::from_canvas(canvas)?);

        Ok(Self {
            surface_impl: Box::new(imple),
            _marker: PhantomData,
        })
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

    pub fn age(&self) -> u8 {
        if self.imp.buffer_presented {
            1
        } else {
            0
        }
    }

    /// Push the buffer to the canvas.
    pub fn present(self) -> Result<(), SoftBufferError> {
        let (width, height) = self
            .imp
            .size
            .expect("Must set size of surface before calling `present()`");
        self.imp.present_with_damage(&[Rect {
            x: 0,
            y: 0,
            width,
            height,
        }])
    }

    pub fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.imp.present_with_damage(damage)
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
