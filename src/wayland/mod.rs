use crate::{error::SwResultExt, util, Rect, SoftBufferError};
use raw_window_handle::{WaylandDisplayHandle, WaylandWindowHandle};
use std::{
    cell::RefCell,
    num::{NonZeroI32, NonZeroU32},
    rc::Rc,
};
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
        let (globals, event_queue) =
            registry_queue_init(&conn).swbuf_err("Failed to make round trip to server")?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .swbuf_err("Failed to instantiate Wayland Shm")?;
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
        let surface_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                window_handle.surface as _,
            )
        }
        .swbuf_err("Failed to create proxy for surface ID.")?;
        let surface = wl_surface::WlSurface::from_id(&display.conn, surface_id)
            .swbuf_err("Failed to create proxy for surface ID.")?;
        Ok(Self {
            display,
            surface,
            buffers: Default::default(),
            size: None,
        })
    }

    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        self.size = Some(
            (|| {
                let width = NonZeroI32::try_from(width).ok()?;
                let height = NonZeroI32::try_from(height).ok()?;
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
            back.resize(width.get(), height.get());
        } else {
            // Allocate front and back buffer
            self.buffers = Some((
                WaylandBuffer::new(
                    &self.display.shm,
                    width.get(),
                    height.get(),
                    &self.display.qh,
                ),
                WaylandBuffer::new(
                    &self.display.shm,
                    width.get(),
                    height.get(),
                    &self.display.qh,
                ),
            ));
        };

        let age = self.buffers.as_mut().unwrap().1.age;
        Ok(BufferImpl {
            stack: util::BorrowStack::new(self, |buffer| {
                Ok(unsafe { buffer.buffers.as_mut().unwrap().1.mapped_mut() })
            })?,
            age,
        })
    }

    /// Fetch the buffer from the window.
    pub fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        Err(SoftBufferError::Unimplemented)
    }

    fn present_with_damage(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let _ = self
            .display
            .event_queue
            .borrow_mut()
            .dispatch_pending(&mut State);

        if let Some((front, back)) = &mut self.buffers {
            // Swap front and back buffer
            std::mem::swap(front, back);

            front.age = 1;
            if back.age != 0 {
                back.age += 1;
            }

            front.attach(&self.surface);

            // Like Mesa's EGL/WSI implementation, we damage the whole buffer with `i32::MAX` if
            // the compositor doesn't support `damage_buffer`.
            // https://bugs.freedesktop.org/show_bug.cgi?id=78190
            if self.surface.version() < 4 {
                self.surface.damage(0, 0, i32::MAX, i32::MAX);
            } else {
                for rect in damage {
                    // Introduced in version 4, it is an error to use this request in version 3 or lower.
                    let (x, y, width, height) = (|| {
                        Some((
                            i32::try_from(rect.x).ok()?,
                            i32::try_from(rect.y).ok()?,
                            i32::try_from(rect.width.get()).ok()?,
                            i32::try_from(rect.height.get()).ok()?,
                        ))
                    })()
                    .ok_or(SoftBufferError::DamageOutOfRange { rect: *rect })?;
                    self.surface.damage_buffer(x, y, width, height);
                }
            }

            self.surface.commit();
        }

        let _ = self.display.event_queue.borrow_mut().flush();

        Ok(())
    }
}

pub struct BufferImpl<'a> {
    stack: util::BorrowStack<'a, WaylandImpl, [u32]>,
    age: u8,
}

impl<'a> BufferImpl<'a> {
    #[inline]
    pub fn pixels(&self) -> &[u32] {
        self.stack.member()
    }

    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.stack.member_mut()
    }

    pub fn age(&self) -> u8 {
        self.age
    }

    pub fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.stack.into_container().present_with_damage(damage)
    }

    pub fn present(self) -> Result<(), SoftBufferError> {
        let imp = self.stack.into_container();
        let (width, height) = imp
            .size
            .expect("Must set size of surface before calling `present()`");
        imp.present_with_damage(&[Rect {
            x: 0,
            y: 0,
            // We know width/height will be non-negative
            width: width.try_into().unwrap(),
            height: height.try_into().unwrap(),
        }])
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
