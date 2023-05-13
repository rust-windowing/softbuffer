use softbuffer::{Context, Surface};
use std::num::NonZeroU32;

use std::panic::{catch_unwind, AssertUnwindSafe};

use winit::event::Event;
use winit::event_loop::{EventLoop, EventLoopWindowTarget};

fn all_red(elwt: &EventLoopWindowTarget<()>) {
    let window = winit::window::WindowBuilder::new()
        .with_title("all_red")
        .with_visible(false)
        .build(elwt)
        .unwrap();

    let context = unsafe { Context::new(elwt) }.unwrap();
    let mut surface = unsafe { Surface::new(&context, &window) }.unwrap();
    let size = window.inner_size();

    // Set the size of the surface to the size of the window.
    surface.resize(
        NonZeroU32::new(size.width).unwrap(),
        NonZeroU32::new(size.height).unwrap(),
    ).unwrap();

    // Set all pixels to red.
    let mut buffer = surface.buffer_mut().unwrap();
    buffer.fill(0xFF0000FF);
    buffer.present().unwrap();

    // Check that all pixels are red.
    let mut buffer = surface.buffer_mut().unwrap();
    buffer.fetch().unwrap();
    for pixel in buffer.iter() {
        assert_eq!(*pixel, 0xFF0000FF);
    }
}

winit_test::main!(all_red);
