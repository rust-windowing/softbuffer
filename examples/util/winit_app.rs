/// Common boilerplate for setting up a winit application.
use std::marker::PhantomData;
use std::rc::Rc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Run a Winit application.
#[allow(unused_mut)]
pub(crate) fn run_app(event_loop: EventLoop<()>, mut app: impl ApplicationHandler<()> + 'static) {
    #[cfg(not(target_family = "wasm"))]
    event_loop.run_app(&mut app).unwrap();

    #[cfg(target_family = "wasm")]
    winit::platform::web::EventLoopExtWebSys::spawn_app(event_loop, app);
}

/// Create a window from a set of window attributes.
#[allow(dead_code)]
pub(crate) fn make_window(
    elwt: &ActiveEventLoop,
    f: impl FnOnce(WindowAttributes) -> WindowAttributes,
) -> Rc<Window> {
    let attributes = f(WindowAttributes::default());
    #[cfg(target_family = "wasm")]
    let attributes = winit::platform::web::WindowAttributesExtWebSys::with_append(attributes, true);
    let window = elwt.create_window(attributes);
    Rc::new(window.unwrap())
}

/// Easily constructable winit application.
pub(crate) struct WinitApp<T, S, Init, InitSurface, Handler, AboutToWaitHandler> {
    /// Closure to initialize `state`.
    init: Init,

    /// Closure to initialize `surface_state`.
    init_surface: InitSurface,

    /// Closure to run on window events.
    event: Handler,

    /// Closure to run on about_to_wait events.
    about_to_wait: AboutToWaitHandler,

    /// Contained state.
    state: Option<T>,

    /// Contained surface state.
    surface_state: Option<S>,
}

/// Builder that makes it so we don't have to name `T`.
pub(crate) struct WinitAppBuilder<T, S, Init, InitSurface> {
    /// Closure to initialize `state`.
    init: Init,

    /// Closure to initialize `surface_state`.
    init_surface: InitSurface,

    /// Eat the type parameter.
    _marker: PhantomData<(Option<T>, Option<S>)>,
}

impl<T, S, Init, InitSurface> WinitAppBuilder<T, S, Init, InitSurface>
where
    Init: FnMut(&ActiveEventLoop) -> T,
    InitSurface: FnMut(&ActiveEventLoop, &mut T) -> S,
{
    /// Create with an "init" closure.
    pub(crate) fn with_init(init: Init, init_surface: InitSurface) -> Self {
        Self {
            init,
            init_surface,
            _marker: PhantomData,
        }
    }

    /// Build a new application.
    #[allow(clippy::type_complexity)]
    pub(crate) fn with_event_handler<F>(
        self,
        handler: F,
    ) -> WinitApp<T, S, Init, InitSurface, F, impl FnMut(&mut T, Option<&mut S>, &ActiveEventLoop)>
    where
        F: FnMut(&mut T, Option<&mut S>, WindowId, WindowEvent, &ActiveEventLoop),
    {
        WinitApp::new(self.init, self.init_surface, handler, |_, _, _| {})
    }
}

impl<T, S, Init, InitSurface, Handler, AboutToWaitHandler>
    WinitApp<T, S, Init, InitSurface, Handler, AboutToWaitHandler>
where
    Init: FnMut(&ActiveEventLoop) -> T,
    InitSurface: FnMut(&ActiveEventLoop, &mut T) -> S,
    Handler: FnMut(&mut T, Option<&mut S>, WindowId, WindowEvent, &ActiveEventLoop),
    AboutToWaitHandler: FnMut(&mut T, Option<&mut S>, &ActiveEventLoop),
{
    /// Create a new application.
    pub(crate) fn new(
        init: Init,
        init_surface: InitSurface,
        event: Handler,
        about_to_wait: AboutToWaitHandler,
    ) -> Self {
        Self {
            init,
            init_surface,
            event,
            about_to_wait,
            state: None,
            surface_state: None,
        }
    }

    /// Build a new application.
    #[allow(dead_code)]
    pub(crate) fn with_about_to_wait_handler<F>(
        self,
        about_to_wait: F,
    ) -> WinitApp<T, S, Init, InitSurface, Handler, F>
    where
        F: FnMut(&mut T, Option<&mut S>, &ActiveEventLoop),
    {
        WinitApp::new(self.init, self.init_surface, self.event, about_to_wait)
    }
}

impl<T, S, Init, InitSurface, Handler, AboutToWaitHandler> ApplicationHandler
    for WinitApp<T, S, Init, InitSurface, Handler, AboutToWaitHandler>
where
    Init: FnMut(&ActiveEventLoop) -> T,
    InitSurface: FnMut(&ActiveEventLoop, &mut T) -> S,
    Handler: FnMut(&mut T, Option<&mut S>, WindowId, WindowEvent, &ActiveEventLoop),
    AboutToWaitHandler: FnMut(&mut T, Option<&mut S>, &ActiveEventLoop),
{
    fn resumed(&mut self, el: &ActiveEventLoop) {
        debug_assert!(self.state.is_none());
        let mut state = (self.init)(el);
        self.surface_state = Some((self.init_surface)(el, &mut state));
        self.state = Some(state);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        let surface_state = self.surface_state.take();
        debug_assert!(surface_state.is_some());
        drop(surface_state);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        let surface_state = self.surface_state.as_mut();
        (self.event)(state, surface_state, window_id, event, event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            let surface_state = self.surface_state.as_mut();
            (self.about_to_wait)(state, surface_state, event_loop);
        }
    }
}
