use raw_window_handle::{HasRawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use tempfile::tempfile;
use wayland_client::{Display, sys::client::wl_display, GlobalManager, protocol::{wl_shm::WlShm, wl_buffer::WlBuffer, wl_surface::WlSurface}, Main, Proxy, EventQueue};
use crate::{GraphicsContextImpl, SoftBufferError, error::unwrap};
use std::{fs::File, os::unix::prelude::{AsRawFd, FileExt}, io::Write};

pub struct WaylandImpl {
    _event_queue: EventQueue,
    surface: WlSurface,
    shm: Main<WlShm>,
    tempfile: File,
    buffer: Option<WaylandBuffer>
}

struct WaylandBuffer{
    width: i32,
    height: i32,
    buffer: Main<WlBuffer>
}

impl WaylandImpl {

    pub unsafe fn new<W: HasRawWindowHandle>(window_handle: WaylandWindowHandle, display_handle: WaylandDisplayHandle) -> Result<Self, SoftBufferError<W>> {
        let display = Display::from_external_display(display_handle.display as *mut wl_display);
        let mut event_queue = display.create_event_queue();
        let attached_display = (*display).clone().attach(event_queue.token());
        let globals = GlobalManager::new(&attached_display);
        unwrap(event_queue.sync_roundtrip(&mut (), |_, _, _| unreachable!()), "Failed to make round trip to server")?;
        let shm = unwrap(globals.instantiate_exact::<WlShm>(1), "Failed to instantiate Wayland Shm")?;
        let tempfile = unwrap(tempfile(), "Failed to create temporary file to store buffer.")?;
        let surface = Proxy::from_c_ptr(window_handle.surface as _).into();
        Ok(Self{
            _event_queue: event_queue,
            surface, shm, tempfile,
            buffer: None
        })
    }

    fn ensure_buffer_size(&mut self, width: i32, height: i32){
        if !self.check_buffer_size_equals(width, height){
            let pool = self.shm.create_pool(self.tempfile.as_raw_fd(), width*height*4);
            let buffer = pool.create_buffer(0, width, height, width*4, wayland_client::protocol::wl_shm::Format::Xrgb8888);
            self.buffer = Some(WaylandBuffer{
                width,
                height,
                buffer
            });
        }
    }

    fn check_buffer_size_equals(&self, width: i32, height: i32) -> bool{
        match &self.buffer{
            Some(buffer) => buffer.width == width && buffer.height == height,
            None => false
        }
    }

}

impl GraphicsContextImpl for WaylandImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        self.ensure_buffer_size(width as i32, height as i32);
        let wayland_buffer = self.buffer.as_mut().unwrap();
        self.tempfile.write_at(std::slice::from_raw_parts(buffer.as_ptr() as *const u8, buffer.len()*4), 0).expect("Failed to write buffer to temporary file.");
        self.tempfile.flush().expect("Failed to flush buffer to temporary file.");
        self.surface.attach(Some(&wayland_buffer.buffer), 0, 0);

        // FIXME: Proper damaging mechanism.
        //
        // In order to propagate changes on compositors which track damage, for now damage the entire surface.
        if self.surface.as_ref().version() < 4 {
            // FIXME: Accommodate scale factor since wl_surface::damage is in terms of surface coordinates while
            // wl_surface::damage_buffer is in buffer coordinates.
            //
            // i32::MAX is a valid damage box (most compositors interpret the damage box as "the entire surface")
            self.surface.damage(0, 0, i32::MAX, i32::MAX);
        } else {
            // Introduced in version 4, it is an error to use this request in version 3 or lower.
            self.surface.damage_buffer(0, 0, width as i32, height as i32);
        }

        self.surface.commit();
    }
}