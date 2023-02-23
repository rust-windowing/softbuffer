use crate::{error::unwrap, util, SoftBufferError};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::{cell::RefCell, num::NonZeroI32, rc::Rc};
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
    event_queue: RefCell<EventQueue<State>>,
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
            event_queue: RefCell::new(event_queue),
            qh,
            shm,
        })
    }
}

pub struct WaylandImpl {
    display: Rc<WaylandDisplayImpl>,
    surface: wl_surface::WlSurface,
    buffers: Option<(WaylandBuffer, WaylandBuffer)>,
    size: Option<(NonZeroI32, NonZeroI32)>,
}

impl WaylandImpl {
    pub unsafe fn new(
        window_handle: WaylandWindowHandle,
        display: Rc<WaylandDisplayImpl>,
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
            size: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SoftBufferError> {
        self.size = Some(
            (|| {
                let width = NonZeroI32::new(i32::try_from(width).ok()?)?;
                let height = NonZeroI32::new(i32::try_from(height).ok()?)?;
                Some((width, height))
            })()
            .ok_or(SoftBufferError::SizeOutOfRange { width, height })?,
        );
        Ok(())
    }

    pub fn buffer_mut(&mut self) -> Result<BufferImpl, SoftBufferError> {
        let (width, height) = self
            .size
            .expect("Must set size of surface before calling `buffer_mut()`");

        if let Some((_front, back)) = &mut self.buffers {
            // Block if back buffer not released yet
            if !back.released() {
                let mut event_queue = self.display.event_queue.borrow_mut();
                while !back.released() {
                    event_queue.blocking_dispatch(&mut State).map_err(|err| {
                        SoftBufferError::PlatformError(
                            Some("Wayland dispatch failure".to_string()),
                            Some(Box::new(err)),
                        )
                    })?;
                }
            }

            // Resize, if buffer isn't large enough
            back.resize(width.into(), height.into());
        } else {
            // Allocate front and back buffer
            self.buffers = Some((
                WaylandBuffer::new(
                    &self.display.shm,
                    width.into(),
                    height.into(),
                    &self.display.qh,
                ),
                WaylandBuffer::new(
                    &self.display.shm,
                    width.into(),
                    height.into(),
                    &self.display.qh,
                ),
            ));
        };

        Ok(BufferImpl(util::BorrowStack::new(self, |buffer| {
            Ok(unsafe { buffer.buffers.as_mut().unwrap().1.mapped_mut() })
        })?))
    }
}

pub struct BufferImpl<'a>(util::BorrowStack<'a, WaylandImpl, [u32]>);

impl<'a> BufferImpl<'a> {
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        self.0.member()
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.0.member_mut()
    }

    pub fn present(self) -> Result<(), SoftBufferError> {
        let imp = self.0.into_container();

        let (width, height) = imp
            .size
            .expect("Must set size of surface before calling `present()`");

        let _ = imp
            .display
            .event_queue
            .borrow_mut()
            .dispatch_pending(&mut State);

        if let Some((front, back)) = &mut imp.buffers {
            // Swap front and back buffer
            std::mem::swap(front, back);

            front.attach(&imp.surface);

            // FIXME: Proper damaging mechanism.
            //
            // In order to propagate changes on compositors which track damage, for now damage the entire surface.
            if imp.surface.version() < 4 {
                // FIXME: Accommodate scale factor since wl_surface::damage is in terms of surface coordinates while
                // wl_surface::damage_buffer is in buffer coordinates.
                //
                // i32::MAX is a valid damage box (most compositors interpret the damage box as "the entire surface")
                imp.surface.damage(0, 0, i32::MAX, i32::MAX);
            } else {
                // Introduced in version 4, it is an error to use this request in version 3 or lower.
                imp.surface.damage_buffer(0, 0, width.into(), height.into());
            }

            imp.surface.commit();
        }

        let _ = imp.display.event_queue.borrow_mut().flush();

        Ok(())
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
