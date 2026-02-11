#![allow(deprecated)] // TODO

#[cfg(not(any(
    target_family = "wasm",
    all(target_vendor = "apple", not(target_os = "macos")),
    target_os = "redox"
)))]
fn buffer(c: &mut criterion::Criterion) {
    use criterion::black_box;
    use softbuffer::{Context, Pixel, Surface};
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

            c.bench_function("next_buffer()", |b| {
                b.iter(|| {
                    black_box(surface.next_buffer().unwrap());
                });
            });

            c.bench_function("pixels()", |b| {
                let mut buffer = surface.next_buffer().unwrap();
                b.iter(|| {
                    let pixels: &mut [Pixel] = buffer.pixels();
                    black_box(pixels);
                });
            });

            c.bench_function("fill pixels", |b| {
                let mut buffer = surface.next_buffer().unwrap();
                b.iter(|| {
                    let buffer = black_box(&mut buffer);
                    buffer.pixels().fill(Pixel::default());
                });
            });

            c.bench_function("render pixels_iter", |b| {
                let mut buffer = surface.next_buffer().unwrap();
                b.iter(|| {
                    let buffer = black_box(&mut buffer);
                    for (x, y, pixel) in buffer.pixels_iter() {
                        let red = (x & 0xff) ^ (y & 0xff);
                        let green = (x & 0x7f) ^ (y & 0x7f);
                        let blue = (x & 0x3f) ^ (y & 0x3f);
                        *pixel = Pixel::new_rgb(red as u8, green as u8, blue as u8);
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
criterion::criterion_group!(benches, buffer);

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
