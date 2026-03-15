use crate::error::InitError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, OrbitalWindowHandle, RawWindowHandle};
use std::{cmp, marker::PhantomData, num::NonZeroU32, slice, str};

use crate::backend_interface::*;
use crate::{util, AlphaMode, Pixel, Rect, SoftBufferError};

#[derive(Debug)]
struct OrbitalMap {
    address: usize,
    size: usize,
    size_unaligned: usize,
}

impl OrbitalMap {
    unsafe fn new(fd: usize, size_unaligned: usize) -> syscall::Result<Self> {
        // Page align size
        let pages = (size_unaligned + syscall::PAGE_SIZE - 1) / syscall::PAGE_SIZE;
        let size = pages * syscall::PAGE_SIZE;

        // Map window buffer
        let address = unsafe {
            syscall::fmap(
                fd,
                &syscall::Map {
                    offset: 0,
                    size,
                    flags: syscall::PROT_READ | syscall::PROT_WRITE | syscall::MAP_SHARED,
                    address: 0,
                },
            )?
        };

        Ok(Self {
            address,
            size,
            size_unaligned,
        })
    }

    unsafe fn data_mut(&mut self) -> &mut [Pixel] {
        unsafe { slice::from_raw_parts_mut(self.address as *mut Pixel, self.size_unaligned / 4) }
    }
}

impl Drop for OrbitalMap {
    fn drop(&mut self) {
        unsafe {
            // Unmap window buffer on drop
            syscall::funmap(self.address, self.size).expect("failed to unmap orbital window");
        }
    }
}

#[derive(Debug)]
pub struct OrbitalImpl<D, W> {
    handle: ThreadSafeWindowHandle,
    width: u32,
    height: u32,
    presented: bool,
    window_handle: W,
    _display: PhantomData<D>,
}

#[derive(Debug)]
struct ThreadSafeWindowHandle(OrbitalWindowHandle);
unsafe impl Send for ThreadSafeWindowHandle {}
unsafe impl Sync for ThreadSafeWindowHandle {}

impl<D: HasDisplayHandle, W: HasWindowHandle> OrbitalImpl<D, W> {
    fn window_fd(&self) -> usize {
        self.handle.0.window.as_ptr() as usize
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for OrbitalImpl<D, W> {
    type Context = D;
    type Buffer<'surface>
        = BufferImpl<'surface>
    where
        Self: 'surface;

    fn new(window: W, _display: &D) -> Result<Self, InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::Orbital(handle) = raw else {
            return Err(InitError::Unsupported(window));
        };

        Ok(Self {
            handle: ThreadSafeWindowHandle(handle),
            width: 0,
            height: 0,
            presented: false,
            window_handle: window,
            _display: PhantomData,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    #[inline]
    fn supports_alpha_mode(&self, alpha_mode: AlphaMode) -> bool {
        matches!(alpha_mode, AlphaMode::Opaque | AlphaMode::Ignored)
    }

    fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        _alpha_mode: AlphaMode,
    ) -> Result<(), SoftBufferError> {
        let width = width.get();
        let height = height.get();
        if width != self.width || height != self.height {
            self.presented = false;
            self.width = width;
            self.height = height;
        }
        Ok(())
    }

    fn next_buffer(&mut self, _alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        let (window_width, window_height) = window_size(self.window_fd());
        let pixels = if self.width as usize == window_width && self.height as usize == window_height
        {
            Pixels::Mapping(
                unsafe { OrbitalMap::new(self.window_fd(), window_width * window_height * 4) }
                    .expect("failed to map orbital window"),
            )
        } else {
            Pixels::Buffer(util::PixelBuffer(vec![
                Pixel::default();
                self.width as usize
                    * self.height as usize
            ]))
        };
        Ok(BufferImpl {
            window_fd: self.window_fd(),
            width: self.width,
            height: self.height,
            presented: &mut self.presented,
            pixels,
        })
    }
}

#[derive(Debug)]
enum Pixels {
    Mapping(OrbitalMap),
    Buffer(util::PixelBuffer),
}

#[derive(Debug)]
pub struct BufferImpl<'surface> {
    window_fd: usize,
    width: u32,
    height: u32,
    presented: &'surface mut bool,
    pixels: Pixels,
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width().get() * 4).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.height as u32).unwrap()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        match &mut self.pixels {
            Pixels::Mapping(mapping) => unsafe { mapping.data_mut() },
            Pixels::Buffer(buffer) => buffer,
        }
    }

    fn age(&self) -> u8 {
        match self.pixels {
            Pixels::Mapping(_) if *self.presented => 1,
            _ => 0,
        }
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        match self.pixels {
            Pixels::Mapping(mapping) => {
                drop(mapping);
                syscall::fsync(self.window_fd).expect("failed to sync orbital window");
                *self.presented = true;
            }
            Pixels::Buffer(buffer) => {
                set_buffer(self.window_fd, &buffer, self.width, self.height, damage);
            }
        }

        Ok(())
    }
}

// Read the current width and size
fn window_size(window_fd: usize) -> (usize, usize) {
    let mut window_width = 0;
    let mut window_height = 0;

    let mut buf: [u8; 4096] = [0; 4096];
    let count = syscall::fpath(window_fd, &mut buf).unwrap();
    let path = str::from_utf8(&buf[..count]).unwrap();
    // orbital:/x/y/w/h/t
    let mut parts = path.split('/').skip(3);
    if let Some(w) = parts.next() {
        window_width = w.parse::<usize>().unwrap_or(0);
    }
    if let Some(h) = parts.next() {
        window_height = h.parse::<usize>().unwrap_or(0);
    }

    (window_width, window_height)
}

fn set_buffer(
    window_fd: usize,
    buffer: &[Pixel],
    width_u32: u32,
    height_u32: u32,
    damage: &[Rect],
) {
    // Read the current width and size
    let (window_width, window_height) = window_size(window_fd);

    {
        // Map window buffer
        let mut window_map =
            unsafe { OrbitalMap::new(window_fd, window_width * window_height * 4) }
                .expect("failed to map orbital window");

        // Window buffer is u32 color data in BGRA format:
        // https://docs.rs/orbclient/0.3.48/src/orbclient/color.rs.html#25-29
        let window_data = unsafe { window_map.data_mut() };

        // Copy each line, cropping to fit
        let width = width_u32 as usize;
        let height = height_u32 as usize;

        // If window size hasn't changed (memory size is same) and we update everything,
        // or if at least one damage rect covers the full window, copy everything at once.
        if width == window_width && (damage.is_empty() || is_full_damage(damage, width, height)) {
            let total_pixels = width * height;
            window_data[..total_pixels].copy_from_slice(&buffer[..total_pixels]);
        } else {
            // Even if width is same, damaged areas can be anywhere inside the window.
            // If they don't cover the full width, we must jump over pixels to copy.
            for rect in damage {
                let start_y = rect.y as usize;
                let rect_height = rect.height.get() as usize;
                let end_y = cmp::min(start_y + rect_height, window_height);

                let rect_x = rect.x as usize;
                let rect_width = rect.width.get() as usize;
                let copy_width = cmp::min(rect_width, window_width.saturating_sub(rect_x));

                // If the rect is exactly the window width and our width hasn't changed,
                // we can copy the rect block without jumping over pixels.
                if copy_width == width && width == window_width {
                    let start_index = start_y * width + rect_x;
                    let total_len = (end_y - start_y) * width;
                    window_data[start_index..start_index + total_len]
                        .copy_from_slice(&buffer[start_index..start_index + total_len]);
                    continue;
                }

                let mut current_buffer_offset = start_y * width + rect_x;
                let mut current_data_offset = start_y * window_width + rect_x;

                // We visit each row of the rect one by one and copy only the specific column range.
                for _ in start_y..end_y {
                    let src = &buffer[current_buffer_offset..current_buffer_offset + copy_width];
                    let dst =
                        &mut window_data[current_data_offset..current_data_offset + copy_width];

                    dst.copy_from_slice(src);

                    current_buffer_offset += width;
                    current_data_offset += window_width;
                }
            }
        }

        // Window buffer map is dropped here
    }

    // Tell orbital to show the latest window data
    syscall::fsync(window_fd).expect("failed to sync orbital window");
}

fn is_full_damage(damage: &[Rect], width: usize, height: usize) -> bool {
    damage.iter().any(|r| {
        r.x == 0 && r.y == 0 && r.width.get() as usize >= width && r.height.get() as usize >= height
    })
}
