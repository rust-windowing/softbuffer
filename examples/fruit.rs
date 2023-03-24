use image::GenericImageView;
use std::rc::Rc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    //see fruit.jpg.license for the license of fruit.jpg
    let fruit = image::load_from_memory(include_bytes!("fruit.jpg")).unwrap();
    let buffer = fruit
        .pixels()
        .map(|(_x, _y, pixel)| {
            let red = pixel.0[0] as u32;
            let green = pixel.0[1] as u32;
            let blue = pixel.0[2] as u32;

            blue | (green << 8) | (red << 16)
        })
        .collect::<Vec<_>>();

    let event_loop = EventLoop::new();
    let window = Rc::new(
        WindowBuilder::new()
            .with_inner_size(winit::dpi::PhysicalSize::new(fruit.width(), fruit.height()))
            .build(&event_loop)
            .unwrap(),
    );

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

    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                surface.set_buffer(&buffer, fruit.width() as u16, fruit.height() as u16);
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
