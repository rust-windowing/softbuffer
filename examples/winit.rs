use std::time::Instant;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use softbuffer::GraphicsContext;

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut graphics_context = unsafe { GraphicsContext::new(window) };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == graphics_context.window().id() => {
                let (width, height) = {
                    let size = graphics_context.window().inner_size();
                    (size.width, size.height)
                };
                let buffer = vec![0x00FF00FF; (width * height) as usize];

                let start = Instant::now();
                graphics_context.set_buffer(&buffer, width as u16, height as u16);
                let elapsed = Instant::now()-start;
                println!("Set in: {}ms", elapsed.as_millis());
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id
            } if window_id == graphics_context.window().id() => {
                *control_flow = ControlFlow::Exit;
            },
            _ => {}
        }
    });
}