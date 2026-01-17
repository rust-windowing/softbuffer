#![allow(deprecated)] // TODO

#[cfg(not(any(
    target_family = "wasm",
    all(target_vendor = "apple", not(target_os = "macos")),
    target_os = "redox"
)))]
fn buffer_mut(c: &mut criterion::Criterion) {
    use criterion::black_box;
    use softbuffer::{Context, Surface};
    use std::num::NonZeroU32;
    use winit::event_loop::ControlFlow;
    use winit::platform::run_on_demand::EventLoopExtRunOnDemand;

    let mut evl = winit::event_loop::EventLoop::new().unwrap();
    let context = Context::new(evl.owned_display_handle()).unwrap();
    let window = evl
        .create_window(winit::window::Window::default_attributes().with_visible(false))
        .unwrap();

    evl.run_on_demand(move |ev, elwt| {
        elwt.set_control_flow(ControlFlow::Poll);

        if let winit::event::Event::AboutToWait = ev {
            elwt.exit();

            let mut surface = Surface::new(&context, &window).unwrap();

            let size = window.inner_size();
            surface
                .resize(
                    NonZeroU32::new(size.width).unwrap(),
                    NonZeroU32::new(size.height).unwrap(),
                )
                .unwrap();

            c.bench_function("buffer_mut()", |b| {
                b.iter(|| {
                    black_box(surface.buffer_mut().unwrap());
                });
            });

            c.bench_function("pixels_mut()", |b| {
                let mut buffer = surface.buffer_mut().unwrap();
                b.iter(|| {
                    let pixels: &mut [u32] = &mut buffer;
                    black_box(pixels);
                });
            });

            c.bench_function("fill", |b| {
                let mut buffer = surface.buffer_mut().unwrap();
                b.iter(|| {
                    let buffer = black_box(&mut buffer);
                    buffer.fill(0x00000000);
                });
            });

            c.bench_function("render", |b| {
                let mut buffer = surface.buffer_mut().unwrap();
                b.iter(|| {
                    let buffer = black_box(&mut buffer);
                    let width = buffer.width().get();
                    for y in 0..buffer.height().get() {
                        for x in 0..buffer.width().get() {
                            let red = (x & 0xff) ^ (y & 0xff);
                            let green = (x & 0x7f) ^ (y & 0x7f);
                            let blue = (x & 0x3f) ^ (y & 0x3f);
                            let value = blue | (green << 8) | (red << 16);
                            buffer[(y * width + x) as usize] = value;
                        }
                    }
                });
            });
        }
    })
    .unwrap();
}

#[cfg(not(any(
    target_family = "wasm",
    all(target_vendor = "apple", not(target_os = "macos")),
    target_os = "redox"
)))]
criterion::criterion_group!(benches, buffer_mut);

#[cfg(not(any(
    target_family = "wasm",
    all(target_vendor = "apple", not(target_os = "macos")),
    target_os = "redox"
)))]
criterion::criterion_main!(benches);

#[cfg(any(
    target_family = "wasm",
    all(target_vendor = "apple", not(target_os = "macos")),
    target_os = "redox"
))]
fn main() {
    panic!("unsupported on WASM, iOS and Redox");
}
