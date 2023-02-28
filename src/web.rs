//! Implementation of software buffering for web targets.

#![allow(clippy::uninlined_format_args)]

use js_sys::Object;
use raw_window_handle::WebWindowHandle;
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::CanvasRenderingContext2d;
use web_sys::HtmlCanvasElement;
use web_sys::ImageData;
use web_sys::OffscreenCanvas;

use crate::error::SwResultExt;
use crate::SoftBufferError;
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

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = OffscreenCanvasRenderingContext2D)]
    pub type OffscreenCanvasRenderingContext2d;

    #[wasm_bindgen(catch, method, structural, js_class = "OffscreenCanvasRenderingContext2D", js_name = putImageData)]
    fn put_image_data(
        this: &OffscreenCanvasRenderingContext2d,
        imagedata: &ImageData,
        dx: f64,
        dy: f64,
    ) -> Result<(), JsValue>;
}

pub struct WebImpl {
    /// The handle and context to the canvas that we're drawing to.
    canvas: Canvas,

    /// The buffer that we're drawing to.
    buffer: Vec<u32>,

    /// The current width of the canvas.
    width: u32,
}

pub enum Canvas {
    Canvas {
        canvas: HtmlCanvasElement,
        ctx: CanvasRenderingContext2d,
    },
    OffscreenCanvas {
        canvas: OffscreenCanvas,
        ctx: OffscreenCanvasRenderingContext2d,
    },
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
        let ctx = Self::resolve_ctx(canvas.get_context("2d").ok(), "CanvasRenderingContext2d")?;

        Ok(Self {
            canvas: Canvas::Canvas { canvas, ctx },
            buffer: Vec::new(),
            width: 0,
        })
    }

    fn from_offscreen_canvas(canvas: OffscreenCanvas) -> Result<Self, SoftBufferError> {
        let ctx = Self::resolve_ctx(
            canvas.get_context("2d").ok(),
            "OffscreenCanvasRenderingContext2d",
        )?;

        Ok(Self {
            canvas: Canvas::OffscreenCanvas { canvas, ctx },
            buffer: Vec::new(),
            width: 0,
        })
    }

    fn resolve_ctx<T: JsCast>(
        result: Option<Option<Object>>,
        name: &str,
    ) -> Result<T, SoftBufferError> {
        let ctx = result
            .swbuf_err("Canvas already controlled using `OffscreenCanvas`")?
            .swbuf_err(
                "A canvas context other than `CanvasRenderingContext2d` was already created",
            )?
            .dyn_into()
            .unwrap_or_else(|_| panic!("`getContext(\"2d\") didn't return a `{name}`"));

        Ok(ctx)
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

/// Extension methods for the Wasm target on [`Surface`](crate::Surface).
pub trait SurfaceExtWeb: Sized {
    /// Creates a new instance of this struct, using the provided [`HtmlCanvasElement`].
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError>;

    /// Creates a new instance of this struct, using the provided [`HtmlCanvasElement`].
    fn from_offscreen_canvas(offscreen_canvas: OffscreenCanvas) -> Result<Self, SoftBufferError>;
}

impl SurfaceExtWeb for crate::Surface {
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError> {
        let imple = crate::SurfaceDispatch::Web(WebImpl::from_canvas(canvas)?);

        Ok(Self {
            surface_impl: Box::new(imple),
            _marker: PhantomData,
        })
    }

    fn from_offscreen_canvas(offscreen_canvas: OffscreenCanvas) -> Result<Self, SoftBufferError> {
        let imple = crate::SurfaceDispatch::Web(WebImpl::from_offscreen_canvas(offscreen_canvas)?);

        Ok(Self {
            surface_impl: Box::new(imple),
            _marker: PhantomData,
        })
    }
}

impl Canvas {
    fn set_width(&self, width: u32) {
        match self {
            Self::Canvas { canvas, .. } => canvas.set_width(width),
            Self::OffscreenCanvas { canvas, .. } => canvas.set_width(width),
        }
    }

    fn set_height(&self, height: u32) {
        match self {
            Self::Canvas { canvas, .. } => canvas.set_height(height),
            Self::OffscreenCanvas { canvas, .. } => canvas.set_height(height),
        }
    }

    fn put_image_data(&self, imagedata: &ImageData, dx: f64, dy: f64) -> Result<(), JsValue> {
        match self {
            Self::Canvas { ctx, .. } => ctx.put_image_data(imagedata, dx, dy),
            Self::OffscreenCanvas { ctx, .. } => ctx.put_image_data(imagedata, dx, dy),
        }
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

        #[cfg(target_feature = "atomics")]
        let result = {
            use js_sys::{Uint8Array, Uint8ClampedArray};

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
            ImageDataExt::new(array, self.imp.width)
                .map(JsValue::from)
                .map(ImageData::unchecked_from_js)
        };
        #[cfg(not(target_feature = "atomics"))]
        let result =
            ImageData::new_with_u8_clamped_array(wasm_bindgen::Clamped(&bitmap), self.imp.width);
        // This should only throw an error if the buffer we pass's size is incorrect.
        let image_data = result.unwrap();

        // This can only throw an error if `data` is detached, which is impossible.
        self.imp
            .canvas
            .put_image_data(&image_data, 0.0, 0.0)
            .unwrap();

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
