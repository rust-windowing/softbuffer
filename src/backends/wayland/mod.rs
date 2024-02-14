use crate::{
    backend_interface::*,
    error::{InitError, SwResultExt},
    util, Rect, SoftBufferError,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
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

pub struct WaylandDisplayImpl<D: ?Sized> {
    conn: Option<Connection>,
    event_queue: RefCell<EventQueue<State>>,
    qh: QueueHandle<State>,
    shm: wl_shm::WlShm,

    /// The object that owns the display handle.
    ///
    /// This has to be dropped *after* the `conn` field, because the `conn` field implicitly borrows
    /// this.
    _display: D,
}

impl<D: HasDisplayHandle + ?Sized> WaylandDisplayImpl<D> {
    fn conn(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }
}

impl<D: HasDisplayHandle + ?Sized> ContextInterface<D> for Rc<WaylandDisplayImpl<D>> {
    fn new(display: D) -> Result<Self, InitError<D>>
    where
        D: Sized,
    {
        let raw = display.display_handle()?.as_raw();
        let wayland_handle = match raw {
            RawDisplayHandle::Wayland(w) => w.display,
            _ => return Err(InitError::Unsupported(display)),
        };

        let backend = unsafe { Backend::from_foreign_display(wayland_handle.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, event_queue) =
            registry_queue_init(&conn).swbuf_err("Failed to make round trip to server")?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .swbuf_err("Failed to instantiate Wayland Shm")?;
        Ok(Rc::new(WaylandDisplayImpl {
            conn: Some(conn),
            event_queue: RefCell::new(event_queue),
            qh,
            shm,
            _display: display,
        }))
    }
}

impl<D: ?Sized> Drop for WaylandDisplayImpl<D> {
    fn drop(&mut self) {
        // Make sure the connection is dropped first.
        self.conn = None;
    }
}

pub struct WaylandImpl<D: ?Sized, W: ?Sized> {
    display: Rc<WaylandDisplayImpl<D>>,
    surface: Option<wl_surface::WlSurface>,
    buffers: Option<(WaylandBuffer, WaylandBuffer)>,
    size: Option<(NonZeroI32, NonZeroI32)>,

    /// The pointer to the window object.
    ///
    /// This has to be dropped *after* the `surface` field, because the `surface` field implicitly
    /// borrows this.
    window_handle: W,
}

impl<D: HasDisplayHandle + ?Sized, W: HasWindowHandle> WaylandImpl<D, W> {
    fn surface(&self) -> &wl_surface::WlSurface {
        self.surface.as_ref().unwrap()
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

            front.attach(self.surface.as_ref().unwrap());

            // Like Mesa's EGL/WSI implementation, we damage the whole buffer with `i32::MAX` if
            // the compositor doesn't support `damage_buffer`.
            // https://bugs.freedesktop.org/show_bug.cgi?id=78190
            if self.surface().version() < 4 {
                self.surface().damage(0, 0, i32::MAX, i32::MAX);
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
                    self.surface().damage_buffer(x, y, width, height);
                }
            }

            self.surface().commit();
        }

        let _ = self.display.event_queue.borrow_mut().flush();

        Ok(())
    }
}

impl<D: HasDisplayHandle + ?Sized, W: HasWindowHandle> SurfaceInterface<D, W>
    for WaylandImpl<D, W>
{
    type Context = Rc<WaylandDisplayImpl<D>>;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    fn new(window: W, display: &Rc<WaylandDisplayImpl<D>>) -> Result<Self, InitError<W>> {
        // Get the raw Wayland window.
        let raw = window.window_handle()?.as_raw();
        let wayland_handle = match raw {
            RawWindowHandle::Wayland(w) => w.surface,
            _ => return Err(InitError::Unsupported(window)),
        };

        let surface_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                wayland_handle.as_ptr().cast(),
            )
        }
        .swbuf_err("Failed to create proxy for surface ID.")?;
        let surface = wl_surface::WlSurface::from_id(display.conn(), surface_id)
            .swbuf_err("Failed to create proxy for surface ID.")?;
        Ok(Self {
            display: display.clone(),
            surface: Some(surface),
            buffers: Default::default(),
            size: None,
            window_handle: window,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
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

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
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
}

impl<D: ?Sized, W: ?Sized> Drop for WaylandImpl<D, W> {
    fn drop(&mut self) {
        // Make sure the surface is dropped first.
        self.surface = None;
    }
}

pub struct BufferImpl<'a, D: ?Sized, W> {
    stack: util::BorrowStack<'a, WaylandImpl<D, W>, [u32]>,
    age: u8,
}

impl<'a, D: HasDisplayHandle + ?Sized, W: HasWindowHandle> BufferInterface
    for BufferImpl<'a, D, W>
{
    #[inline]
    fn pixels(&self) -> &[u32] {
        self.stack.member()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        self.stack.member_mut()
    }

    fn age(&self) -> u8 {
        self.age
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.stack.into_container().present_with_damage(damage)
    }

    fn present(self) -> Result<(), SoftBufferError> {
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
