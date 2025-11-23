use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle};
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::console::gop::{BltOp, BltPixel, BltRegion, GraphicsOutput, PixelFormat};

use crate::backend_interface::*;
use crate::error::{InitError, SwResultExt};
use crate::{util, Rect, SoftBufferError};
use std::marker::PhantomData;
use std::num::NonZeroU32;

#[derive(Clone)]
pub struct UefiDisplayImpl<D> {
    display: D,
}

impl<D: HasDisplayHandle> ContextInterface<D> for UefiDisplayImpl<D> {
    fn new(display: D) -> Result<Self, InitError<D>> {
        let raw = display.display_handle()?.as_raw();
        let RawDisplayHandle::Uefi(..) = raw else {
            return Err(InitError::Unsupported(display));
        };

        Ok(Self { display })
    }
}

pub struct UefiImpl<D, W> {
    /// The current canvas width/height.
    size: Option<(NonZeroU32, NonZeroU32)>,

    /// The buffer that we're drawing to.
    buffer: Vec<u32>,

    /// Buffer has been presented.
    buffer_presented: bool,

    /// The underlying window handle.
    window_handle: W,

    /// The underlying display handle.
    _display: PhantomData<D>,

    /// The UEFI protocol for graphics output.
    proto: boot::ScopedProtocol<GraphicsOutput>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> UefiImpl<D, W> {
    fn present_with_damage(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let (buffer_width, _buffer_height) = self
            .size
            .expect("Must set size of surface before calling `present_with_damage()`");

        let union_damage = if let Some(rect) = util::union_damage(damage) {
            rect
        } else {
            return Ok(());
        };

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
            .map(|pixel| ((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8))
            .collect();

        debug_assert_eq!(
            bitmap.len() as u32,
            union_damage.width.get() * union_damage.height.get() * 4
        );

        let mode = self.proto.current_mode_info();
        if mode.pixel_format() == PixelFormat::BltOnly {
            self.proto
                .blt(BltOp::BufferToVideo {
                    buffer: &bitmap
                        .into_iter()
                        .map(|pixel| BltPixel::new(pixel.0, pixel.1, pixel.2))
                        .collect::<Vec<_>>(),
                    src: BltRegion::Full,
                    dest: (union_damage.x as usize, union_damage.y as usize),
                    dims: (
                        union_damage.width.get() as usize,
                        union_damage.height.get() as usize,
                    ),
                })
                .swbuf_err("Failed to blt buffer")?;
        } else {
            let mut fb = self.proto.frame_buffer();
            unsafe {
                fb.write_value(
                    (union_damage.y as usize) * (union_damage.width.get() as usize) + mode.stride(),
                    bitmap
                        .into_iter()
                        .flat_map(|pixel| [pixel.0, pixel.1, pixel.2, 255])
                        .collect::<Vec<_>>(),
                );
            }
        }

        self.buffer_presented = true;

        Ok(())
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for UefiImpl<D, W> {
    type Context = UefiDisplayImpl<D>;
    type Buffer<'a>
        = BufferImpl<'a, D, W>
    where
        Self: 'a;

    fn new(window_handle: W, display: &UefiDisplayImpl<D>) -> Result<Self, InitError<W>> {
        let raw = display.display.display_handle()?.as_raw();
        let RawDisplayHandle::Uefi(display) = raw else {
            return Err(InitError::Failure(SoftBufferError::IncompleteDisplayHandle));
        };

        let proto = unsafe {
            boot::open_protocol::<GraphicsOutput>(
                OpenProtocolParams {
                    handle: uefi::data_types::Handle::new(display.handle),
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        }
        .swbuf_err("Failed to open the graphics output protocol")?;

        Ok(Self {
            size: None,
            buffer: Vec::new(),
            buffer_presented: false,
            window_handle,
            _display: PhantomData,
            proto,
        })
    }

    /// Get the inner window handle.
    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        if self.size != Some((width, height)) {
            self.buffer_presented = false;
            self.buffer.resize(total_len(width.get(), height.get()), 0);
            self.size = Some((width, height));
        }

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        Ok(BufferImpl { imp: self })
    }

    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Ok(self.buffer.clone())
    }
}

pub struct BufferImpl<'a, D, W> {
    imp: &'a mut UefiImpl<D, W>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'_, D, W> {
    #[inline]
    fn pixels(&self) -> &[u32] {
        &self.imp.buffer
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.imp.buffer
    }

    #[inline]
    fn age(&self) -> u8 {
        if self.imp.buffer_presented {
            1
        } else {
            0
        }
    }

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
