use crate::{Format, Rect, SoftBufferError};
use std::num::NonZeroU32;

pub struct ConvertFormat {
    buffer: Vec<u32>,
    buffer_presented: bool,
    size: Option<(NonZeroU32, NonZeroU32)>,
    in_fmt: Format,
    out_fmt: Format,
}

impl ConvertFormat {
    pub fn new(in_fmt: Format, out_fmt: Format) -> Result<Self, SoftBufferError> {
        // TODO select out_fmt from native_formats?
        Ok(Self {
            buffer: Vec::new(),
            buffer_presented: false,
            size: None,
            in_fmt,
            out_fmt,
        })
    }

    pub fn pixels(&self) -> &[u32] {
        &self.buffer
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }

    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) {
        if self.size == Some((width, height)) {
            return;
        }
        self.size = Some((width, height));
        self.buffer_presented = false;
        self.buffer
            .resize((u32::from(width) * u32::from(height)) as usize, 0);
    }

    pub fn age(&self) -> u8 {
        if self.buffer_presented {
            1
        } else {
            0
        }
    }

    // Can only damage be copied? Need to track damage for multiple buffers? if backend uses
    pub fn present(&self, outputs: &mut [u32], damage: &[Rect]) {
        assert_eq!(outputs.len(), self.buffer.len());
        convert_format(self.in_fmt, &self.buffer, self.out_fmt, outputs);
        self.buffer_presented;
    }
}

fn convert_pixels<F: FnMut([u8; 4]) -> [u8; 4]>(inputs: &[u32], outputs: &mut [u32], mut cb: F) {
    for (input, output) in inputs.iter().zip(outputs.iter_mut()) {
        *output = u32::from_ne_bytes(cb(input.to_ne_bytes()));
    }
}

// Convert between BGR* and RGB*
#[inline(always)]
fn swap_rb(mut bytes: [u8; 4]) -> [u8; 4] {
    bytes.swap(0, 2);
    bytes
}

// Convert ***X to ***A format by setting alpha to 255
#[inline(always)]
fn set_opaque(mut bytes: [u8; 4]) -> [u8; 4] {
    bytes[3] = 255;
    bytes
}

fn convert_format(in_fmt: Format, inputs: &[u32], out_fmt: Format, outputs: &mut [u32]) {
    use Format::*;
    match (in_fmt, out_fmt) {
        (RGBA, RGBA) | (RGBX, RGBX) | (BGRA, BGRA) | (BGRX, BGRX) => {
            outputs.copy_from_slice(inputs)
        }
        (RGBX, RGBA) | (BGRX, BGRA) => convert_pixels(inputs, outputs, set_opaque),
        (RGBX, BGRX) | (RGBA, BGRA) | (BGRX, RGBX) | (BGRA, RGBA) => {
            convert_pixels(inputs, outputs, swap_rb)
        }
        (RGBX, BGRA) | (BGRX, RGBA) => convert_pixels(inputs, outputs, |x| set_opaque(swap_rb(x))),
        (RGBA | BGRA, RGBX | BGRX) => unimplemented!("can't convert alpha to non-alpha format"),
    }
}
