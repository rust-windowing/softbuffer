#![allow(deprecated)] // TODO

use criterion::{criterion_group, criterion_main, Criterion};

fn buffer_mut(c: &mut Criterion) {
    #[cfg(target_family = "wasm")]
    {
        // Do nothing.
        let _ = c;
    }

    #[cfg(not(target_family = "wasm"))]
    {
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
                        for _ in 0..500 {
                            black_box(surface.buffer_mut().unwrap());
                        }
                    });
                });

                c.bench_function("pixels_mut()", |b| {
                    let mut buffer = surface.buffer_mut().unwrap();
                    b.iter(|| {
                        for _ in 0..500 {
                            let x: &mut [u32] = &mut buffer;
                            black_box(x);
                        }
                    });
                });
            }
        })
        .unwrap();
    }
}

criterion_group!(benches, buffer_mut);
criterion_main!(benches);
