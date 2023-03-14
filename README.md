# Softbuffer - Library for displaying pixel buffers in pure Rust.

[![Crates.io](https://img.shields.io/crates/v/softbuffer.svg)](https://crates.io/crates/softbuffer)
[![Docs.rs](https://docs.rs/softbuffer/badge.svg)](https://docs.rs/softbuffer)
[![CI Status](https://github.com/rust-windowing/softbuffer/workflows/CI/badge.svg)](https://github.com/rust-windowing/softbuffer/actions)

Softbuffer allows displaying a 2D pixel buffer on a window in a cross-platform way.

Softbuffer integrates with [raw-window-handle](https://crates.io/crates/raw-window-handle). This means softbuffer
does not depend on any particular windowing library and can be used with any of the high quality windowing
libraries such as [winit](https://crates.io/crates/winit).

## Example
```rust,no_run
// This example uses winit.
//
// Any other library which supports raw-window-handle can also be used.
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    // Create a window with winit.
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    // A context communicates with the display server to initialize any state
    // shared between windows.
    //
    // This must not outlive the display server.
    let context = unsafe { softbuffer::Context::new(&window) }.unwrap();

    // A surface is used to display a buffer on the window.
    //
    // This is created per window and the surface must not outlive the window and context.
    let mut surface = unsafe { softbuffer::Surface::new(&context, &window) }.unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };

                // Create a gradient to display in the window.
                //
                // You might use a drawing library such as tiny-skia instead of
                // directly setting pixel colors and reuse this buffer if a resize
                // has not occured.
                let buffer = (0..((width * height) as usize))
                    .map(|index| {
                        let y = index / (width as usize);
                        let x = index % (width as usize);
                        let red = x % 255;
                        let green = y % 255;
                        let blue = (x * y) % 255;

                        let color = blue | (green << 8) | (red << 16);

                        color as u32
                    })
                    .collect::<Vec<_>>();

                // Display the buffer on the window.
                //
                // The width and height should be determined by using your windowing library.
                surface.set_buffer(&buffer, width as u16, height as u16);
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
```

## Platform support

Not every platform is supported, that [raw-window-handle](https://crates.io/crates/raw-window-handle) supports. Pull requests are welcome to add new platforms!

**Nonetheless, all major desktop platforms that winit uses on desktop are supported.**

For now, the priority for new platforms is:
1. At least one platform for each OS (e.g. one of Win32 or WinRT, or one of Xlib, Xcb, and Wayland)
2. Any other platform supported by winit.
3. Any other platform

| Platform | ✅ (Present) ❌ (Absent) |
|-|-|
| AndroidNdk | ❌ |
| AppKit | ✅ |
| Orbital | ✅ |
| UiKit | ❌ |
| Wayland | ✅ |
| Web | ✅ |
| Win32 | ✅ |
| WinRt | ❌ |
| Xcb | ❌ |
| Xlib | ✅ |

### Thanks to

- AppKit: [Seo Sanghyeon](https://github.com/sanxiyn) and [lunixbochs](https://github.com/lunixbochs)
- Web: [Liamolucko](https://github.com/Liamolucko)

## WebAssembly

To run an example with the web backend: `cargo run-wasm --example winit`

## License & Credits

This library is dual-licensed under MIT or Apache-2.0, just like minifb and rust. Significant portions of code were taken
from the minifb for the platform-specific implementation.

## Comparison with other crates

### [minifb](https://crates.io/crates/minifb)

minifb is the primary inspiration for this crate. It is easy to present some pixels with minifb but minifb
has quite limited window management capabilities. Softbuffer integrates better with the rust windowing ecosystem
and does not force a specific window management scheme.

### [pixels](https://crates.io/crates/pixels)

Pixels has a similar goal to softbuffer but includes some features that may not be nessecary if the goal is to display
a buffer and nothing else. pixels provides GPU post processing and therefore requires a GPU. This makes pixels unsuitable for
systems where hardware accelerated graphics are not available (e.g. VMs, older computers, computers with
misconfigured or work in progress drivers). Softbuffer should be used over pixels when its GPU-accelerated
post-processing effects are not needed.

# Changelog

See the [changelog](CHANGELOG.md) for a list of this package's versions and the changes made in each version.
