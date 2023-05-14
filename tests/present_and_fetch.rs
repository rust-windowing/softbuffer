use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::event_loop::EventLoopWindowTarget;

fn all_red(elwt: &EventLoopWindowTarget<()>) {
    let window = winit::window::WindowBuilder::new()
        .with_title("all_red")
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

    // winit does not wait for the window to be mapped... sigh
    std::thread::sleep(std::time::Duration::from_millis(1));

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
    buffer.fill(0xFF000000);
    buffer.present().unwrap();

    // Check that all pixels are red.
    let screen_contents = surface.fetch().unwrap();
    for pixel in screen_contents.iter() {
        assert_eq!(*pixel, 0xFF000000);
    }
}

winit_test::main!(all_red);
