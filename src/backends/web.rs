//! Implementation of software buffering for web targets.

#![allow(clippy::uninlined_format_args)]

use js_sys::Object;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::ImageData;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

use crate::backend_interface::*;
use crate::error::{InitError, SwResultExt};
use crate::{util, NoDisplayHandle, NoWindowHandle, Rect, SoftBufferError};
use std::convert::TryInto;
use std::marker::PhantomData;
use std::num::NonZeroU32;

/// Display implementation for the web platform.
///
/// This just caches the document to prevent having to query it every time.
pub struct WebDisplayImpl<D> {
    document: web_sys::Document,
    _display: D,
}

impl<D: HasDisplayHandle> ContextInterface<D> for WebDisplayImpl<D> {
    fn new(display: D) -> Result<Self, InitError<D>> {
        let raw = display.display_handle()?.as_raw();
        match raw {
            RawDisplayHandle::Web(..) => {}
            _ => return Err(InitError::Unsupported(display)),
        }

        let document = web_sys::window()
            .swbuf_err("`Window` is not present in this runtime")?
            .document()
            .swbuf_err("`Document` is not present in this runtime")?;

        Ok(Self {
            document,
            _display: display,
        })
    }
}

pub struct WebImpl<D, W> {
    /// The handle and context to the canvas that we're drawing to.
    canvas: Canvas,

    /// The buffer that we're drawing to.
    buffer: Vec<u32>,

    /// Buffer has been presented.
    buffer_presented: bool,

    /// The current canvas width/height.
    size: Option<(NonZeroU32, NonZeroU32)>,

    /// The underlying window handle.
    window_handle: W,

    /// The underlying display handle.
    _display: PhantomData<D>,
}

/// Holding canvas and context for [`HtmlCanvasElement`] or [`OffscreenCanvas`],
/// since they have different types.
enum Canvas {
    Canvas {
        canvas: HtmlCanvasElement,
        ctx: CanvasRenderingContext2d,
    },
    OffscreenCanvas {
        canvas: OffscreenCanvas,
        ctx: OffscreenCanvasRenderingContext2d,
    },
}

impl<D: HasDisplayHandle, W: HasWindowHandle> WebImpl<D, W> {
    fn from_canvas(canvas: HtmlCanvasElement, window: W) -> Result<Self, SoftBufferError> {
        let ctx = Self::resolve_ctx(canvas.get_context("2d").ok(), "CanvasRenderingContext2d")?;

        Ok(Self {
            canvas: Canvas::Canvas { canvas, ctx },
            buffer: Vec::new(),
            buffer_presented: false,
            size: None,
            window_handle: window,
            _display: PhantomData,
        })
    }

    fn from_offscreen_canvas(canvas: OffscreenCanvas, window: W) -> Result<Self, SoftBufferError> {
        let ctx = Self::resolve_ctx(
            canvas.get_context("2d").ok(),
            "OffscreenCanvasRenderingContext2d",
        )?;

        Ok(Self {
            canvas: Canvas::OffscreenCanvas { canvas, ctx },
            buffer: Vec::new(),
            buffer_presented: false,
            size: None,
            window_handle: window,
            _display: PhantomData,
        })
    }

    fn resolve_ctx<T: JsCast>(
        result: Option<Option<Object>>,
        name: &str,
    ) -> Result<T, SoftBufferError> {
        let ctx = result
            .swbuf_err("Canvas already controlled using `OffscreenCanvas`")?
            .swbuf_err(format!(
                "A canvas context other than `{name}` was already created"
            ))?
            .dyn_into()
            .unwrap_or_else(|_| panic!("`getContext(\"2d\") didn't return a `{name}`"));

        Ok(ctx)
    }

    fn present_with_damage(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let (buffer_width, _buffer_height) = self
            .size
            .expect("Must set size of surface before calling `present_with_damage()`");

        let union_damage = if let Some(rect) = util::union_damage(damage) {
            rect
        } else {
            return Ok(());
        };

        // Create a bitmap from the buffer.
        let bitmap: Vec<_> = self
            .buffer
            .chunks_exact(buffer_width.get() as usize)
            .skip(union_damage.y as usize)
            .take(union_damage.height.get() as usize)
            .flat_map(|row| {
                row.iter()
                    .skip(union_damage.x as usize)
                    .take(union_damage.width.get() as usize)
            })
            .copied()
            .flat_map(|pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 255])
            .collect();

        debug_assert_eq!(
            bitmap.len() as u32,
            union_damage.width.get() * union_damage.height.get() * 4
        );

        #[cfg(target_feature = "atomics")]
        let result = {
            // When using atomics, the underlying memory becomes `SharedArrayBuffer`,
            // which can't be shared with `ImageData`.
            use js_sys::{Uint8Array, Uint8ClampedArray};
            use wasm_bindgen::prelude::wasm_bindgen;

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
            ImageDataExt::new(array, union_damage.width.get())
                .map(JsValue::from)
                .map(ImageData::unchecked_from_js)
        };
        #[cfg(not(target_feature = "atomics"))]
        let result = ImageData::new_with_u8_clamped_array(
            wasm_bindgen::Clamped(&bitmap),
            union_damage.width.get(),
        );
        // This should only throw an error if the buffer we pass's size is incorrect.
        let image_data = result.unwrap();

        for rect in damage {
            // This can only throw an error if `data` is detached, which is impossible.
            self.canvas
                .put_image_data(
                    &image_data,
                    union_damage.x.into(),
                    union_damage.y.into(),
                    (rect.x - union_damage.x).into(),
                    (rect.y - union_damage.y).into(),
                    rect.width.get().into(),
                    rect.height.get().into(),
                )
                .unwrap();
        }

        self.buffer_presented = true;

        Ok(())
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for WebImpl<D, W> {
    type Context = WebDisplayImpl<D>;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    fn new(window: W, display: &WebDisplayImpl<D>) -> Result<Self, InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let canvas: HtmlCanvasElement = match raw {
            RawWindowHandle::Web(handle) => {
                display
                    .document
                    .query_selector(&format!("canvas[data-raw-handle=\"{}\"]", handle.id))
                    // `querySelector` only throws an error if the selector is invalid.
                    .unwrap()
                    .swbuf_err("No canvas found with the given id")?
                    // We already made sure this was a canvas in `querySelector`.
                    .unchecked_into()
            }
            RawWindowHandle::WebCanvas(handle) => {
                let value: &JsValue = unsafe { handle.obj.cast().as_ref() };
                value.clone().unchecked_into()
            }
            RawWindowHandle::WebOffscreenCanvas(handle) => {
                let value: &JsValue = unsafe { handle.obj.cast().as_ref() };
                let canvas: OffscreenCanvas = value.clone().unchecked_into();

                return Self::from_offscreen_canvas(canvas, window).map_err(InitError::Failure);
            }
            _ => return Err(InitError::Unsupported(window)),
        };

        Self::from_canvas(canvas, window).map_err(InitError::Failure)
    }

    /// Get the inner window handle.
    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    /// De-duplicates the error handling between `HtmlCanvasElement` and `OffscreenCanvas`.
    /// Resize the canvas to the given dimensions.
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        if self.size != Some((width, height)) {
            self.buffer_presented = false;
            self.buffer.resize(total_len(width.get(), height.get()), 0);
            self.canvas.set_width(width.get());
            self.canvas.set_height(height.get());
            self.size = Some((width, height));
        }

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        Ok(BufferImpl { imp: self })
    }

    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        let (width, height) = self
            .size
            .expect("Must set size of surface before calling `fetch()`");

        let image_data = self
            .canvas
            .get_image_data(0., 0., width.get().into(), height.get().into())
            .ok()
            // TODO: Can also error if width or height are 0.
            .swbuf_err("`Canvas` contains pixels from a different origin")?;

        Ok(image_data
            .data()
            .0
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes([0, chunk[0], chunk[1], chunk[2]]))
            .collect())
    }
}

/// Extension methods for the Wasm target on [`Surface`](crate::Surface).
pub trait SurfaceExtWeb: Sized {
    /// Creates a new instance of this struct, using the provided [`HtmlCanvasElement`].
    ///
    /// # Errors
    /// - If the canvas was already controlled by an `OffscreenCanvas`.
    /// - If a another context then "2d" was already created for this canvas.
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError>;

    /// Creates a new instance of this struct, using the provided [`OffscreenCanvas`].
    ///
    /// # Errors
    /// If a another context then "2d" was already created for this canvas.
    fn from_offscreen_canvas(offscreen_canvas: OffscreenCanvas) -> Result<Self, SoftBufferError>;
}

impl SurfaceExtWeb for crate::Surface<NoDisplayHandle, NoWindowHandle> {
    fn from_canvas(canvas: HtmlCanvasElement) -> Result<Self, SoftBufferError> {
        let imple = crate::SurfaceDispatch::Web(WebImpl::from_canvas(canvas, NoWindowHandle(()))?);

        Ok(Self {
            surface_impl: Box::new(imple),
            _marker: PhantomData,
        })
    }

    fn from_offscreen_canvas(offscreen_canvas: OffscreenCanvas) -> Result<Self, SoftBufferError> {
        let imple = crate::SurfaceDispatch::Web(WebImpl::from_offscreen_canvas(
            offscreen_canvas,
            NoWindowHandle(()),
        )?);

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

    fn get_image_data(&self, sx: f64, sy: f64, sw: f64, sh: f64) -> Result<ImageData, JsValue> {
        match self {
            Canvas::Canvas { ctx, .. } => ctx.get_image_data(sx, sy, sw, sh),
            Canvas::OffscreenCanvas { ctx, .. } => ctx.get_image_data(sx, sy, sw, sh),
        }
    }

    // NOTE: suppress the lint because we mirror `CanvasRenderingContext2D.putImageData()`, and
    // this is just an internal API used by this module only, so it's not too relevant.
    #[allow(clippy::too_many_arguments)]
    fn put_image_data(
        &self,
        imagedata: &ImageData,
        dx: f64,
        dy: f64,
        dirty_x: f64,
        dirty_y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), JsValue> {
        match self {
            Self::Canvas { ctx, .. } => ctx
                .put_image_data_with_dirty_x_and_dirty_y_and_dirty_width_and_dirty_height(
                    imagedata, dx, dy, dirty_x, dirty_y, width, height,
                ),
            Self::OffscreenCanvas { ctx, .. } => ctx
                .put_image_data_with_dirty_x_and_dirty_y_and_dirty_width_and_dirty_height(
                    imagedata, dx, dy, dirty_x, dirty_y, width, height,
                ),
        }
    }
}

pub struct BufferImpl<'a, D, W> {
    imp: &'a mut WebImpl<D, W>,
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'a, D, W> {
    fn pixels(&self) -> &[u32] {
        &self.imp.buffer
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.imp.buffer
    }

    fn age(&self) -> u8 {
        if self.imp.buffer_presented {
            1
        } else {
            0
        }
    }

    /// Push the buffer to the canvas.
    fn present(self) -> Result<(), SoftBufferError> {
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

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
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
