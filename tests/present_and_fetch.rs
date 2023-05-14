use softbuffer::{Context, Surface};

use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};

use winit::event::Event;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};

struct WinitBasedTest {
    name: &'static str,
    test_fn: fn(&EventLoopWindowTarget<()>),
}

fn all_red(elwt: &EventLoopWindowTarget<()>) {
    let window = winit::window::WindowBuilder::new()
        .with_title("all_red")
        .with_visible(false)
        .build(elwt)
        .unwrap();

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowExtWebSys;

        web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .body()
            .unwrap()
            .append_child(&window.canvas())
            .unwrap();
    }

    let context = unsafe { Context::new(elwt) }.unwrap();
    let mut surface = unsafe { Surface::new(&context, &window) }.unwrap();
    let size = window.inner_size();

    // Set the size of the surface to the size of the window.
    surface
        .resize(
            NonZeroU32::new(size.width).unwrap(),
            NonZeroU32::new(size.height).unwrap(),
        )
        .unwrap();

    // Set all pixels to red.
    let mut buffer = surface.buffer_mut().unwrap();
    buffer.fill(0x000000FF);
    buffer.present().unwrap();

    // Check that all pixels are red.
    let screen_contents = surface.fetch().unwrap();
    for pixel in screen_contents.iter() {
        assert_eq!(*pixel, 0x000000FF);
    }
}

const TESTS: &[WinitBasedTest] = &[WinitBasedTest {
    name: "all_red",
    test_fn: all_red,
}];

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    EventLoop::new().run(run)
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn main() {
    use winit::platform::web::EventLoopExtWebSys;

    EventLoop::new().spawn(run);
}

fn run(ev: Event<'_, ()>, elwt: &EventLoopWindowTarget<()>, ctrl: &mut ControlFlow) {
    ctrl.set_poll();

    if let Event::Resumed = ev {
        // We can now create windows; run tests!
        for test in TESTS {
            print!("Running test {}...", test.name);
            match catch_unwind(AssertUnwindSafe(move || (test.test_fn)(elwt))) {
                Ok(()) => println!(" OK!"),
                Err(e) => {
                    println!(" FAILED!");

                    if let Some(s) = e.downcast_ref::<&'static str>() {
                        println!("    {}", s);
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        println!("    {}", s);
                    } else {
                        println!("    <unknown panic type>");
                    }

                    ctrl.set_exit_with_code(1);
                }
            }
        }

        ctrl.set_exit();
    }
}
