use crate::{error::unwrap, GraphicsContextImpl, SwBufError};
use nix::sys::memfd::{memfd_create, MemFdCreateFlag};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::{
    ffi::CStr,
    fs::File,
    io::Write,
    os::unix::prelude::{AsRawFd, FileExt, FromRawFd},
};
use wayland_client::{
    backend::{Backend, ObjectId},
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool, wl_surface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

struct State;

pub struct WaylandImpl {
    event_queue: EventQueue<State>,
    qh: QueueHandle<State>,
    surface: wl_surface::WlSurface,
    shm: wl_shm::WlShm,
    tempfile: File,
    buffer: Option<WaylandBuffer>,
}

struct WaylandBuffer {
    width: i32,
    height: i32,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
}

impl Drop for WaylandBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
    }
}

impl WaylandImpl {
    pub unsafe fn new(
        window_handle: WaylandWindowHandle,
        display_handle: WaylandDisplayHandle,
    ) -> Result<Self, SwBufError> {
        let conn = Connection::from_backend(Backend::from_foreign_display(
            display_handle.display as *mut _,
        ));
        let (globals, event_queue) = unwrap(
            registry_queue_init(&conn),
            "Failed to make round trip to server",
        )?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = unwrap(
            globals.bind(&qh, 1..=1, ()),
            "Failed to instantiate Wayland Shm",
        )?;
        let name = CStr::from_bytes_with_nul_unchecked("swbuf\0".as_bytes());
        let tempfile_fd = unwrap(
            memfd_create(name, MemFdCreateFlag::MFD_CLOEXEC),
            "Failed to create temporary file to store buffer.",
        )?;
        let tempfile = File::from_raw_fd(tempfile_fd);
        let surface_id = unwrap(
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                window_handle.surface as _,
            ),
            "Failed to create proxy for surface ID.",
        )?;
        let surface = unwrap(
            wl_surface::WlSurface::from_id(&conn, surface_id),
            "Failed to create proxy for surface ID.",
        )?;
        Ok(Self {
            event_queue: event_queue,
            qh,
            surface,
            shm,
            tempfile,
            buffer: None,
        })
    }

    fn ensure_buffer_size(&mut self, width: i32, height: i32) {
        if !self.check_buffer_size_equals(width, height) {
            let pool =
                self.shm
                    .create_pool(self.tempfile.as_raw_fd(), width * height * 4, &self.qh, ());
            let buffer = pool.create_buffer(
                0,
                width,
                height,
                width * 4,
                wayland_client::protocol::wl_shm::Format::Xrgb8888,
                &self.qh,
                (),
            );
            self.buffer = Some(WaylandBuffer {
                width,
                height,
                pool,
                buffer,
            });
        }
    }

    fn check_buffer_size_equals(&self, width: i32, height: i32) -> bool {
        match &self.buffer {
            Some(buffer) => buffer.width == width && buffer.height == height,
            None => false,
        }
    }
}

impl GraphicsContextImpl for WaylandImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        self.ensure_buffer_size(width as i32, height as i32);
        let wayland_buffer = self.buffer.as_mut().unwrap();
        self.tempfile.set_len(buffer.len() as u64 * 4)
            .expect("Failed to truncate temporary file.");
        self.tempfile
            .write_at(
                std::slice::from_raw_parts(buffer.as_ptr() as *const u8, buffer.len() * 4),
                0,
            )
            .expect("Failed to write buffer to temporary file.");
        self.tempfile
            .flush()
            .expect("Failed to flush buffer to temporary file.");
        self.surface.attach(Some(&wayland_buffer.buffer), 0, 0);

        // FIXME: Proper damaging mechanism.
        //
        // In order to propagate changes on compositors which track damage, for now damage the entire surface.
        if self.surface.version() < 4 {
            // FIXME: Accommodate scale factor since wl_surface::damage is in terms of surface coordinates while
            // wl_surface::damage_buffer is in buffer coordinates.
            //
            // i32::MAX is a valid damage box (most compositors interpret the damage box as "the entire surface")
            self.surface.damage(0, 0, i32::MAX, i32::MAX);
        } else {
            // Introduced in version 4, it is an error to use this request in version 3 or lower.
            self.surface
                .damage_buffer(0, 0, width as i32, height as i32);
        }

        self.surface.commit();
        let _ = self.event_queue.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut State,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        // Ignore globals added after initialization
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn event(
        _: &mut State,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for State {
    fn event(
        _: &mut State,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn event(
        _: &mut State,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}
