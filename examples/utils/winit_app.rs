/// Common boilerplate for setting up a winit application.
use std::marker::PhantomData;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Run a Winit application.
#[allow(unused_mut)]
pub(crate) fn run_app(event_loop: EventLoop<()>, mut app: impl ApplicationHandler<()> + 'static) {
    #[cfg(not(any(target_arch = "wasm32", target_arch = "wasm64")))]
    event_loop.run_app(&mut app).unwrap();

    #[cfg(any(target_arch = "wasm32", target_arch = "wasm64"))]
    winit::platform::web::EventLoopExtWebSys::spawn_app(event_loop, app);
}

/// Create a window from a set of window attributes.
#[allow(dead_code)]
pub(crate) fn make_window(
    elwt: &ActiveEventLoop,
    f: impl FnOnce(WindowAttributes) -> WindowAttributes,
) -> Rc<Window> {
    let attributes = f(WindowAttributes::default());
    #[cfg(target_arch = "wasm32")]
    let attributes = winit::platform::web::WindowAttributesExtWebSys::with_append(attributes, true);
    let window = elwt.create_window(attributes);
    Rc::new(window.unwrap())
}

/// Easily constructable winit application.
pub(crate) struct WinitApp<T, Init, Handler> {
    /// Closure to initialize state.
    init: Init,

    /// Closure to run on window events.
    event: Handler,

    /// Contained state.
    state: Option<T>,
}

/// Builder that makes it so we don't have to name `T`.
pub(crate) struct WinitAppBuilder<T, Init> {
    /// Closure to initialize state.
    init: Init,

    /// Eat the type parameter.
    _marker: PhantomData<Option<T>>,
}

impl<T, Init> WinitAppBuilder<T, Init>
where
    Init: FnMut(&ActiveEventLoop) -> T,
{
    /// Create with an "init" closure.
    pub(crate) fn with_init(init: Init) -> Self {
        Self {
            init,
            _marker: PhantomData,
        }
    }

    /// Build a new application.
    pub(crate) fn with_event_handler<F>(self, handler: F) -> WinitApp<T, Init, F>
    where
        F: FnMut(&mut T, Event<()>, &ActiveEventLoop),
    {
        WinitApp::new(self.init, handler)
    }
}

impl<T, Init, Handler> WinitApp<T, Init, Handler>
where
    Init: FnMut(&ActiveEventLoop) -> T,
    Handler: FnMut(&mut T, Event<()>, &ActiveEventLoop),
{
    /// Create a new application.
    pub(crate) fn new(init: Init, event: Handler) -> Self {
        Self {
            init,
            event,
            state: None,
        }
    }
}

impl<T, Init, Handler> ApplicationHandler for WinitApp<T, Init, Handler>
where
    Init: FnMut(&ActiveEventLoop) -> T,
    Handler: FnMut(&mut T, Event<()>, &ActiveEventLoop),
{
    fn resumed(&mut self, el: &ActiveEventLoop) {
        debug_assert!(self.state.is_none());
        self.state = Some((self.init)(el));
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        let state = self.state.take();
        debug_assert!(state.is_some());
        drop(state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        (self.event)(state, Event::WindowEvent { window_id, event }, event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            (self.event)(state, Event::AboutToWait, event_loop);
        }
    }
}
