use crate::error::InitError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, OrbitalWindowHandle, RawWindowHandle};
use std::{marker::PhantomData, num::NonZeroU32, slice, str};

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
                Pixel::INIT;
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
    _height_u32: u32,
    damage: &[Rect],
) {
    // Read the current width and size
    let (window_width, window_height) = window_size(window_fd);

    let Some(urect) = util::union_damage(damage) else {
        syscall::fsync(window_fd).expect("failed to sync orbital window");
        return;
    };

    {
        // Map window buffer
        let mut window_map =
            unsafe { OrbitalMap::new(window_fd, window_width * window_height * 4) }
                .expect("failed to map orbital window");

        // Window buffer is u32 color data in BGRA format:
        // https://docs.rs/orbclient/0.3.48/src/orbclient/color.rs.html#25-29
        let window_data = unsafe { window_map.data_mut() };

        let width = width_u32 as usize;

        let x = urect.x as usize;
        let y = urect.y as usize;
        let w = (urect.width.get() as usize).min(window_width.saturating_sub(x));
        let h = (urect.height.get() as usize).min(window_height.saturating_sub(y));

        for (src_row, dst_row) in buffer
            .chunks_exact(width)
            .zip(window_data.chunks_exact_mut(window_width))
            .skip(y)
            .take(h)
        {
            dst_row[x..x + w].copy_from_slice(&src_row[x..x + w]);
        }

        // Window buffer map is dropped here
    }

    // Tell orbital to show the latest window data
    syscall::fsync(window_fd).expect("failed to sync orbital window");
}
