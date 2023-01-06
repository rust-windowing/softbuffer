use crate::{error::unwrap, SoftBufferError};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::sync::{Arc, Mutex};
use wayland_client::{
    backend::{Backend, ObjectId},
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_shm, wl_surface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

mod buffer;
use buffer::WaylandBuffer;

struct State;

pub struct WaylandDisplayImpl {
    conn: Connection,
    event_queue: Mutex<EventQueue<State>>,
    qh: QueueHandle<State>,
    shm: wl_shm::WlShm,
}

impl WaylandDisplayImpl {
    pub unsafe fn new(display_handle: WaylandDisplayHandle) -> Result<Self, SoftBufferError> {
        // SAFETY: Ensured by user
        let backend = unsafe { Backend::from_foreign_display(display_handle.display as *mut _) };
        let conn = Connection::from_backend(backend);
        let (globals, event_queue) = unwrap(
            registry_queue_init(&conn),
            "Failed to make round trip to server",
        )?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = unwrap(
            globals.bind(&qh, 1..=1, ()),
            "Failed to instantiate Wayland Shm",
        )?;
        Ok(Self {
            conn,
            event_queue: Mutex::new(event_queue),
            qh,
            shm,
        })
    }
}

pub struct WaylandImpl {
    display: Arc<WaylandDisplayImpl>,
    surface: wl_surface::WlSurface,
    buffers: Option<(WaylandBuffer, WaylandBuffer)>,
}

impl WaylandImpl {
    pub unsafe fn new(
        window_handle: WaylandWindowHandle,
        display: Arc<WaylandDisplayImpl>,
    ) -> Result<Self, SoftBufferError> {
        // SAFETY: Ensured by user
        let surface_id = unwrap(
            unsafe {
                ObjectId::from_ptr(
                    wl_surface::WlSurface::interface(),
                    window_handle.surface as _,
                )
            },
            "Failed to create proxy for surface ID.",
        )?;
        let surface = unwrap(
            wl_surface::WlSurface::from_id(&display.conn, surface_id),
            "Failed to create proxy for surface ID.",
        )?;
        Ok(Self {
            display,
            surface,
            buffers: Default::default(),
        })
    }

    fn buffer(&mut self, width: i32, height: i32) -> &WaylandBuffer {
        self.buffers = Some(if let Some((front, mut back)) = self.buffers.take() {
            // Swap buffers; block if back buffer not released yet
            if !back.released() {
                let mut event_queue = self.display.event_queue.lock().unwrap();
                while !back.released() {
                    event_queue.blocking_dispatch(&mut State).unwrap();
                }
            }
            back.resize(width, height);
            (back, front)
        } else {
            // Allocate front and back buffer
            (
                WaylandBuffer::new(&self.display.shm, width, height, &self.display.qh),
                WaylandBuffer::new(&self.display.shm, width, height, &self.display.qh),
            )
        });
        &self.buffers.as_ref().unwrap().0
    }

    pub(super) unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16) {
        let _ = self
            .display
            .event_queue
            .lock()
            .unwrap()
            .dispatch_pending(&mut State);

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

        let _ = self.display.event_queue.lock().unwrap().flush();
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
