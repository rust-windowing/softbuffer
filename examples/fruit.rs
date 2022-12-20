use image::GenericImageView;
use swbuf::GraphicsContext;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    //see fruit.jpg.license for the license of fruit.jpg
    let fruit = image::load_from_memory(include_bytes!("fruit.jpg")).unwrap();
    let buffer = fruit.pixels().map(|(_x, _y, pixel)|{
        let red = pixel.0[0] as u32;
        let green = pixel.0[1] as u32;
        let blue = pixel.0[2] as u32;

        blue | (green << 8) | (red << 16)
    }).collect::<Vec<_>>();

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

    let mut graphics_context = unsafe { GraphicsContext::new(&window) }.unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                graphics_context.set_buffer(&buffer, fruit.width() as u16, fruit.height() as u16);
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
