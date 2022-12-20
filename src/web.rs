use raw_window_handle::WebWindowHandle;
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::CanvasRenderingContext2d;
use web_sys::HtmlCanvasElement;
use web_sys::ImageData;

use crate::GraphicsContextImpl;
use crate::SwBufError;

pub struct WebImpl {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
}

impl WebImpl {
    pub fn new(handle: WebWindowHandle) -> Result<Self, SwBufError> {
        let canvas: HtmlCanvasElement = web_sys::window()
            .ok_or_else(|| {
                SwBufError::PlatformError(
                    Some("`window` is not present in this runtime".into()),
                    None,
                )
            })?
            .document()
            .ok_or_else(|| {
                SwBufError::PlatformError(
                    Some("`document` is not present in this runtime".into()),
                    None,
                )
            })?
            .query_selector(&format!("canvas[data-raw-handle=\"{}\"]", handle.id))
            // `querySelector` only throws an error if the selector is invalid.
            .unwrap()
            .ok_or_else(|| {
                SwBufError::PlatformError(
                    Some("No canvas found with the given id".into()),
                    None,
                )
            })?
            // We already made sure this was a canvas in `querySelector`.
            .unchecked_into();

        let ctx = canvas
        .get_context("2d")
        .map_err(|_| {
            SwBufError::PlatformError(
                Some("Canvas already controlled using `OffscreenCanvas`".into()),
                None,
            )
        })?
        .ok_or_else(|| {
            SwBufError::PlatformError(
                Some("A canvas context other than `CanvasRenderingContext2d` was already created".into()),
                None,
            )
        })?
        .dyn_into()
        .expect("`getContext(\"2d\") didn't return a `CanvasRenderingContext2d`");

        Ok(Self { canvas, ctx })
    }
}

impl GraphicsContextImpl for WebImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        self.canvas.set_width(width.into());
        self.canvas.set_height(height.into());

        let bitmap: Vec<_> = buffer
            .iter()
            .copied()
            .flat_map(|pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 255])
            .collect();

        // This should only throw an error if the buffer we pass's size is incorrect, which is checked in the outer `set_buffer` call.
        let image_data =
            ImageData::new_with_u8_clamped_array(Clamped(&bitmap), width.into()).unwrap();

        // This can only throw an error if `data` is detached, which is impossible.
        self.ctx.put_image_data(&image_data, 0.0, 0.0).unwrap();
    }
}
