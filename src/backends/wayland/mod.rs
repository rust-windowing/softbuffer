use crate::{
    backend_interface::*,
    error::{InitError, SwResultExt},
    util, AlphaMode, Pixel, Rect, SoftBufferError,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::{
    num::{NonZeroI32, NonZeroU32},
    sync::{Arc, Mutex},
};
use wayland_client::{
    backend::{Backend, ObjectId},
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_fixes, wl_registry, wl_shm, wl_surface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};

mod buffer;
use buffer::WaylandBuffer;

struct State;

#[derive(Debug)]
pub struct WaylandDisplayImpl<D: ?Sized> {
    conn: Option<Connection>,
    event_queue: Mutex<EventQueue<State>>,
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

impl<D: HasDisplayHandle + ?Sized> ContextInterface<D> for Arc<WaylandDisplayImpl<D>> {
    fn new(display: D) -> Result<Self, InitError<D>>
    where
        D: Sized,
    {
        let raw = display.display_handle()?.as_raw();
        let RawDisplayHandle::Wayland(w) = raw else {
            return Err(InitError::Unsupported(display));
        };

        let backend = unsafe { Backend::from_foreign_display(w.display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        let (globals, event_queue) =
            registry_queue_init(&conn).swbuf_err("Failed to make round trip to server")?;
        let qh = event_queue.handle();
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .swbuf_err("Failed to instantiate Wayland Shm")?;

        // If `wl_fixes` is supported, destroy registry using it.
        // We don't need the registry anymore.
        if let Ok(fixes) = globals.bind::<wl_fixes::WlFixes, _, ()>(&qh, 1..=1, ()) {
            fixes.destroy_registry(globals.registry());
            conn.backend()
                .destroy_object(&globals.registry().id())
                .unwrap();
            fixes.destroy();
        }

        Ok(Arc::new(WaylandDisplayImpl {
            conn: Some(conn),
            event_queue: Mutex::new(event_queue),
            qh,
            shm,
            _display: display,
        }))
    }
}

impl<D: ?Sized> Drop for WaylandDisplayImpl<D> {
    fn drop(&mut self) {
        if self.shm.version() >= 2 {
            self.shm.release();
        }
        // Make sure the connection is dropped first.
        self.conn = None;
    }
}

#[derive(Debug)]
pub struct WaylandImpl<D: ?Sized, W: ?Sized> {
    display: Arc<WaylandDisplayImpl<D>>,
    surface: Option<wl_surface::WlSurface>,
    buffers: Option<(WaylandBuffer, WaylandBuffer)>,
    size: Option<(NonZeroI32, NonZeroI32)>,
    /// The pointer to the window object.
    ///
    /// This has to be dropped *after* the `surface` field, because the `surface` field implicitly
    /// borrows this.
    window_handle: W,
}

impl<D: HasDisplayHandle + ?Sized, W: HasWindowHandle> SurfaceInterface<D, W>
    for WaylandImpl<D, W>
{
    type Context = Arc<WaylandDisplayImpl<D>>;
    type Buffer<'surface>
        = BufferImpl<'surface>
    where
        Self: 'surface;

    fn new(window: W, display: &Arc<WaylandDisplayImpl<D>>) -> Result<Self, InitError<W>> {
        // Get the raw Wayland window.
        let raw = window.window_handle()?.as_raw();
        let RawWindowHandle::Wayland(w) = raw else {
            return Err(InitError::Unsupported(window));
        };

        let surface_id = unsafe {
            ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                w.surface.as_ptr().cast(),
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

    #[inline]
    fn supports_alpha_mode(&self, alpha_mode: AlphaMode) -> bool {
        matches!(
            alpha_mode,
            AlphaMode::Opaque | AlphaMode::Ignored | AlphaMode::Premultiplied
        )
    }

    fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
        _alpha_mode: AlphaMode,
    ) -> Result<(), SoftBufferError> {
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

    fn next_buffer(&mut self, alpha_mode: AlphaMode) -> Result<BufferImpl<'_>, SoftBufferError> {
        // This is documented as `0xXXRRGGBB` on a little-endian machine, which means a byte
        // order of `[B, G, R, X]`.
        let format = match alpha_mode {
            AlphaMode::Opaque | AlphaMode::Ignored => wl_shm::Format::Xrgb8888,
            AlphaMode::Premultiplied => wl_shm::Format::Argb8888,
            _ => unimplemented!(),
        };

        let (width, height) = self
            .size
            .expect("Must set size of surface before calling `next_buffer()`");

        if let Some((_front, back)) = &mut self.buffers {
            // Block if back buffer not released yet
            if !back.released() {
                let mut event_queue = self
                    .display
                    .event_queue
                    .lock()
                    .unwrap_or_else(|x| x.into_inner());
                while !back.released() {
                    event_queue.blocking_dispatch(&mut State).map_err(|err| {
                        SoftBufferError::PlatformError(
                            Some("Wayland dispatch failure".to_string()),
                            Some(Box::new(err)),
                        )
                    })?;
                }
            }

            // Resize if buffer isn't large enough.
            back.configure(width.get(), height.get(), format);
        } else {
            // Allocate front and back buffer
            self.buffers = Some((
                WaylandBuffer::new(
                    &self.display.shm,
                    width.get(),
                    height.get(),
                    format,
                    &self.display.qh,
                ),
                WaylandBuffer::new(
                    &self.display.shm,
                    width.get(),
                    height.get(),
                    format,
                    &self.display.qh,
                ),
            ));
        };

        let (front, back) = self.buffers.as_mut().unwrap();

        let width = back.width;
        let height = back.height;
        let age = back.age;
        Ok(BufferImpl {
            event_queue: &self.display.event_queue,
            surface: self.surface.as_ref().unwrap(),
            front,
            back,
            width,
            height,
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

#[derive(Debug)]
pub struct BufferImpl<'surface> {
    event_queue: &'surface Mutex<EventQueue<State>>,
    surface: &'surface wl_surface::WlSurface,
    front: &'surface mut WaylandBuffer,
    back: &'surface mut WaylandBuffer,
    width: i32,
    height: i32,
    age: u8,
}

impl BufferInterface for BufferImpl<'_> {
    fn byte_stride(&self) -> NonZeroU32 {
        NonZeroU32::new(util::byte_stride(self.width as u32)).unwrap()
    }

    fn width(&self) -> NonZeroU32 {
        NonZeroU32::new(self.width as u32).unwrap()
    }

    fn height(&self) -> NonZeroU32 {
        NonZeroU32::new(self.height as usize as u32).unwrap()
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [Pixel] {
        self.back.mapped_mut()
    }

    fn age(&self) -> u8 {
        self.age
    }

    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let _ = self
            .event_queue
            .lock()
            .unwrap_or_else(|x| x.into_inner())
            .dispatch_pending(&mut State);

        // Swap front and back buffer
        std::mem::swap(self.front, self.back);

        self.front.age = 1;
        if self.back.age != 0 {
            self.back.age += 1;
        }

        self.front.attach(self.surface);

        // Like Mesa's EGL/WSI implementation, we damage the whole buffer with `i32::MAX` if
        // the compositor doesn't support `damage_buffer`.
        // https://bugs.freedesktop.org/show_bug.cgi?id=78190
        if self.surface.version() < 4 {
            self.surface.damage(0, 0, i32::MAX, i32::MAX);
        } else {
            for rect in damage {
                // Damage that falls outside the surface is ignored, so we don't need to clamp the
                // rect manually.
                // https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_surface
                let x = util::to_i32_saturating(rect.x);
                let y = util::to_i32_saturating(rect.y);
                let width = util::to_i32_saturating(rect.width.get());
                let height = util::to_i32_saturating(rect.height.get());

                // Introduced in version 4, it is an error to use this request in version 3 or lower.
                self.surface.damage_buffer(x, y, width, height);
            }
        }

        self.surface.commit();

        let _ = self
            .event_queue
            .lock()
            .unwrap_or_else(|x| x.into_inner())
            .flush();

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

impl Dispatch<wl_fixes::WlFixes, ()> for State {
    fn event(
        _: &mut State,
        _: &wl_fixes::WlFixes,
        _: wl_fixes::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}
