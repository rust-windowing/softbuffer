use raw_window_handle::OrbitalWindowHandle;
use std::{cmp, slice, str};

use crate::SwBufError;

struct OrbitalMap {
    address: usize,
    size: usize,
}

impl OrbitalMap {
    unsafe fn new(fd: usize, size_unaligned: usize) -> syscall::Result<Self> {
        // Page align size
        let pages = (size_unaligned + syscall::PAGE_SIZE - 1) / syscall::PAGE_SIZE;
        let size = pages * syscall::PAGE_SIZE;

        // Map window buffer
        let address = syscall::fmap(
            fd,
            &syscall::Map {
                offset: 0,
                size,
                flags: syscall::PROT_READ | syscall::PROT_WRITE,
                address: 0,
            },
        )?;

        Ok(Self { address, size })
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

pub struct OrbitalImpl {
    handle: OrbitalWindowHandle,
}

impl OrbitalImpl {
    pub fn new(handle: OrbitalWindowHandle) -> Result<Self, SwBufError> {
        Ok(Self { handle })
    }

    pub(crate) unsafe fn set_buffer(&mut self, buffer: &[u32], width_u16: u16, height_u16: u16) {
        let window_fd = self.handle.window as usize;

        // Read the current width and size
        let mut window_width = 0;
        let mut window_height = 0;
        {
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
        }

        {
            // Map window buffer
            let window_map = OrbitalMap::new(window_fd, window_width * window_height * 4)
                .expect("failed to map orbital window");

            // Window buffer is u32 color data in 0xAABBGGRR format
            let window_data = slice::from_raw_parts_mut(
                window_map.address as *mut u32,
                window_width * window_height,
            );

            // Copy each line, cropping to fit
            let width = width_u16 as usize;
            let height = height_u16 as usize;
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
        syscall::fsync(window_fd).expect("failed to sync orbital window");
    }
}
