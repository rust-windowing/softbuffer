use crate::{error::unwrap, GraphicsContextImpl, SwBufError};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::collections::VecDeque;
use wayland_client::{
    backend::{Backend, ObjectId},
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_shm, wl_surface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

mod buffer;
use buffer::WaylandBuffer;

struct State;

pub struct WaylandImpl {
    event_queue: EventQueue<State>,
    qh: QueueHandle<State>,
    surface: wl_surface::WlSurface,
    shm: wl_shm::WlShm,
    // 0-2 buffers
    buffers: VecDeque<WaylandBuffer>,
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
            buffers: Default::default(),
        })
    }

    // Allocate or reuse a buffer of the given size
    fn buffer(&mut self, width: i32, height: i32) -> &WaylandBuffer {
        let buffer = if let Some(mut buffer) = self.buffers.pop_front() {
            if buffer.released() {
                buffer.resize(width, height);
                buffer
            } else {
                // If we have more than 1 unreleased buffer, destroy it
                if self.buffers.len() == 0 {
                    self.buffers.push_back(buffer);
                }
                WaylandBuffer::new(&self.shm, width, height, &self.qh)
            }
        } else {
            WaylandBuffer::new(&self.shm, width, height, &self.qh)
        };
        self.buffers.push_back(buffer);
        self.buffers.back().unwrap()
    }
}

impl GraphicsContextImpl for WaylandImpl {
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let _ = self.event_queue.dispatch_pending(&mut State);

        let surface = self.surface.clone();
        let wayland_buffer = self.buffer(width.into(), height.into());
        wayland_buffer.write(buffer);
        wayland_buffer.attach(&surface);

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
