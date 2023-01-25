use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

const BUFFER_WIDTH: usize = 256;
const BUFFER_HEIGHT: usize = 128;

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

    let context = unsafe { softbuffer::Context::new(&window) }.unwrap();
    let mut surface = unsafe { softbuffer::Surface::new(&context, &window) }.unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                surface.resize(BUFFER_WIDTH as u32, BUFFER_HEIGHT as u32);

                let buffer = surface.buffer_mut();
                for y in 0..BUFFER_HEIGHT {
                    for x in 0..BUFFER_WIDTH {
                        let red = x as u32 % 255;
                        let green = y as u32 % 255;
                        let blue = (x as u32 * y as u32) % 255;

                        let color = blue | (green << 8) | (red << 16);
                        buffer[y * BUFFER_WIDTH + x] = color;
                    }
                }

                surface.present().unwrap();
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
