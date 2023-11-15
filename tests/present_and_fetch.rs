use softbuffer::{Context, Surface};
use std::num::NonZeroU32;
use winit::event_loop::EventLoopWindowTarget;

fn all_red(elwt: &EventLoopWindowTarget<()>) {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_mut))]
    let mut builder = winit::window::WindowBuilder::new().with_title("all_red");

    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::WindowBuilderExtWebSys;

        builder = builder.with_append(true);
    }

    let window = builder.build(elwt).unwrap();

    // winit does not wait for the window to be mapped... sigh
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(1));

    let context = Context::new(elwt).unwrap();
    let mut surface = Surface::new(&context, &window).unwrap();
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
    buffer.fill(0x00FF0000);
    buffer.present().unwrap();

    // Check that all pixels are red.
    let screen_contents = match surface.fetch() {
        Err(softbuffer::SoftBufferError::Unimplemented) => return,
        cont => cont.unwrap(),
    };
    for pixel in screen_contents.iter() {
        assert_eq!(*pixel, 0x00FF0000);
    }
}

winit_test::main!(all_red);
