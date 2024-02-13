use crate::error::InitError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, OrbitalWindowHandle, RawWindowHandle};
use std::{cmp, marker::PhantomData, num::NonZeroU32, slice, str};

use crate::backend_interface::*;
use crate::{Rect, SoftBufferError};

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

    unsafe fn data(&self) -> &[u32] {
        unsafe { slice::from_raw_parts(self.address as *const u32, self.size_unaligned / 4) }
    }

    unsafe fn data_mut(&self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(self.address as *mut u32, self.size_unaligned / 4) }
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

pub struct OrbitalImpl<D, W> {
    handle: OrbitalWindowHandle,
    width: u32,
    height: u32,
    presented: bool,
    window_handle: W,
    _display: PhantomData<D>,
}

impl<D: HasDisplayHandle, W: HasWindowHandle> OrbitalImpl<D, W> {
    fn window_fd(&self) -> usize {
        self.handle.window.as_ptr() as usize
    }

    // Read the current width and size
    fn window_size(&self) -> (usize, usize) {
        let mut window_width = 0;
        let mut window_height = 0;

        let mut buf: [u8; 4096] = [0; 4096];
        let count = syscall::fpath(self.window_fd(), &mut buf).unwrap();
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

    fn set_buffer(&self, buffer: &[u32], width_u32: u32, height_u32: u32) {
        // Read the current width and size
        let (window_width, window_height) = self.window_size();

        {
            // Map window buffer
            let window_map =
                unsafe { OrbitalMap::new(self.window_fd(), window_width * window_height * 4) }
                    .expect("failed to map orbital window");

            // Window buffer is u32 color data in 0xAABBGGRR format
            let window_data = unsafe { window_map.data_mut() };

            // Copy each line, cropping to fit
            let width = width_u32 as usize;
            let height = height_u32 as usize;
            let min_width = cmp::min(width, window_width);
            let min_height = cmp::min(height, window_height);
            for y in 0..min_height {
                let offset_buffer = y * width;
                let offset_data = y * window_width;
                window_data[offset_data..offset_data + min_width]
                    .copy_from_slice(&buffer[offset_buffer..offset_buffer + min_width]);
            }

            // Window buffer map is dropped here
        }

        // Tell orbital to show the latest window data
        syscall::fsync(self.window_fd()).expect("failed to sync orbital window");
    }
}

impl<D: HasDisplayHandle, W: HasWindowHandle> SurfaceInterface<D, W> for OrbitalImpl<D, W> {
    type Context = D;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    fn new(window: W, _display: &D) -> Result<Self, InitError<W>> {
        let raw = window.window_handle()?.as_raw();
        let handle = match raw {
            RawWindowHandle::Orbital(handle) => handle,
            _ => return Err(InitError::Unsupported(window)),
        };

        Ok(Self {
            handle,
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

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        let width = width.get();
        let height = height.get();
        if width != self.width || height != self.height {
            self.presented = false;
            self.width = width;
            self.height = height;
        }
        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        let (window_width, window_height) = self.window_size();
        let pixels = if self.width as usize == window_width && self.height as usize == window_height
        {
            Pixels::Mapping(
                unsafe { OrbitalMap::new(self.window_fd(), window_width * window_height * 4) }
                    .expect("failed to map orbital window"),
            )
        } else {
            Pixels::Buffer(vec![0; self.width as usize * self.height as usize])
        };
        Ok(BufferImpl { imp: self, pixels })
    }
}

enum Pixels {
    Mapping(OrbitalMap),
    Buffer(Vec<u32>),
}

pub struct BufferImpl<'a, D, W> {
    imp: &'a mut OrbitalImpl<D, W>,
    pixels: Pixels,
}

impl<'a, D: HasDisplayHandle, W: HasWindowHandle> BufferInterface for BufferImpl<'a, D, W> {
    #[inline]
    fn pixels(&self) -> &[u32] {
        match &self.pixels {
            Pixels::Mapping(mapping) => unsafe { mapping.data() },
            Pixels::Buffer(buffer) => buffer,
        }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        match &mut self.pixels {
            Pixels::Mapping(mapping) => unsafe { mapping.data_mut() },
            Pixels::Buffer(buffer) => buffer,
        }
    }

    fn age(&self) -> u8 {
        match self.pixels {
            Pixels::Mapping(_) if self.imp.presented => 1,
            _ => 0,
        }
    }

    fn present(self) -> Result<(), SoftBufferError> {
        match self.pixels {
            Pixels::Mapping(mapping) => {
                drop(mapping);
                syscall::fsync(self.imp.window_fd()).expect("failed to sync orbital window");
                self.imp.presented = true;
            }
            Pixels::Buffer(buffer) => {
                self.imp
                    .set_buffer(&buffer, self.imp.width, self.imp.height);
            }
        }

        Ok(())
    }

    fn present_with_damage(self, _damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.present()
    }
}
