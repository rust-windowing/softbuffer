use std::f64::consts::PI;
use instant::Instant;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use swbuf::GraphicsContext;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

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

    let mut graphics_context = unsafe { GraphicsContext::new(&window, &window) }.unwrap();

    let mut old_size = (0, 0);
    let mut frames = pre_render_frames(0, 0);

    let start = Instant::now();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                let elapsed = start.elapsed().as_secs_f64() % 1.0;
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };

                if (width, height) != old_size{
                    old_size = (width, height);
                    frames = pre_render_frames(width as usize, height as usize);
                };

                let buffer = &frames[((elapsed*60.0).round() as usize).clamp(0, 59)];
                graphics_context.set_buffer(buffer.as_slice(), width as u16, height as u16);
            }
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}

fn pre_render_frames(width: usize, height: usize) -> Vec<Vec<u32>>{
    let render = |frame_id|{
        let elapsed = ((frame_id as f64)/(60.0))*2.0*PI;
        let buffer = (0..((width * height) as usize))
            .map(|index| {
                let y = ((index / (width as usize)) as f64)/(height as f64);
                let x = ((index % (width as usize)) as f64)/(width as f64);
                let red = ((((y + elapsed).sin()*0.5+0.5)*255.0).round() as u32).clamp(0, 255);
                let green = ((((x + elapsed).sin()*0.5+0.5)*255.0).round() as u32).clamp(0, 255);
                let blue = ((((y - elapsed).cos()*0.5+0.5)*255.0).round() as u32).clamp(0, 255);

                let color = blue | (green << 8) | (red << 16);

                color
            })
            .collect::<Vec<_>>();

        buffer
    };

    #[cfg(target_arch = "wasm32")]
    return (0..60).map(render).collect();

    #[cfg(not(target_arch = "wasm32"))]
    (0..60).into_par_iter().map(render).collect()
}
