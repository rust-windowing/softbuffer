Overview
==
As the popularity of the library [minifb](https://crates.io/crates/minifb) shows, it is useful to put a 2D buffer/image
on a window in a platform-independent way. Minifb's approach to doing window management itself, however, is problematic
code duplication. We already have very high quality libraries for this in the Rust ecosystem
(such as [winit](https://crates.io/crates/winit)), and minifb's implementation of window management is not ideal. For
example, it occasionally segfaults on some platforms and is missing key features such as the ability to set a window
icon. While it would be possible to add these features to minifb, it makes more sense to instead use the standard
window handling systems.

Softbuffer integrates with the [raw-window-handle](https://crates.io/crates/raw-window-handle) crate
to allow writing to a window in a cross-platform way while using the very high quality dedicated window management
libraries that are available in the Rust ecosystem.

What about [pixels](https://crates.io/crates/pixels)? Pixels accomplishes a very similar goal to Softbuffer,
however there are two key differences. Pixels provides some capacity for GPU-accelerated post-processing of what is
displayed, while Softbuffer does not. Due to not having this post-processing, Softbuffer does not rely on the GPU or
hardware accelerated graphics stack in any way, and is thus more portable to installations that do not have access to
hardware acceleration (e.g. VMs, older computers, computers with misconfigured drivers). Softbuffer should be used over
pixels when its GPU-accelerated post-processing effects are not needed.


License & Credits
==

This library is dual-licensed under MIT or Apache-2.0, just like minifb and rust. Significant portions of code were taken
from the minifb library to do platform-specific work.

Platform support:
==
Some, but not all, platforms supported in [raw-window-handle](https://crates.io/crates/raw-window-handle) are supported
by Softbuffer. Pull requests are welcome to add new platforms! **Nonetheless, all major desktop platforms that winit uses
on desktop are supported.**

For now, the priority for new platforms is:
1) to have at least one platform on each OS working (e.g. one of Win32 or WinRT, or one of Xlib, Xcb, and Wayland) and
2) for that one platform on each OS to be the one that winit uses.

(PRs will be accepted for any platform, even if it does not follow the above priority.)

|  Platform ||
|-----------|--|
|Android NDK|❌|
|   AppKit  |✅|
|  Orbital  |✅|
|    UIKit  |✅|
|  Wayland  |✅|
|    Web    |✅|
|   Win32   |✅|
|   WinRT   |❌|
|    XCB    |✅|
|   Xlib    |✅|

✅: Present\
❔: Immature\
❌: Absent

WebAssembly
-----------

To run an example with the web backend: `cargo run-wasm --example winit`

Example
==
```rust,no_run
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

#[path = "../examples/utils/winit_app.rs"]
mod winit_app;

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let mut app = winit_app::WinitAppBuilder::with_init(|elwt| {
        let window = {
            let window = elwt.create_window(Window::default_attributes());
            Rc::new(window.unwrap())
        };
        let context = softbuffer::Context::new(window.clone()).unwrap();
        let surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        (window, surface)
    }).with_event_handler(|state, event, elwt| {
        let (window, surface) = state;
        elwt.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent { window_id, event: WindowEvent::RedrawRequested } if window_id == window.id() => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };
                surface
                    .resize(
                        NonZeroU32::new(width).unwrap(),
                        NonZeroU32::new(height).unwrap(),
                    )
                    .unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                for index in 0..(width * height) {
                    let y = index / width;
                    let x = index % width;
                    let red = x % 255;
                    let green = y % 255;
                    let blue = (x * y) % 255;

                    buffer[index as usize] = blue | (green << 8) | (red << 16);
                }

                buffer.present().unwrap();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => {
                elwt.exit();
            }
            _ => {}
        }
    });

    event_loop.run_app(&mut app).unwrap();
}
```

MSRV Policy
==
This crate's Minimum Supported Rust Version (MSRV) is **1.70**. Changes to
the MSRV will be accompanied by a minor version bump.

As a **tentative** policy, the upper bound of the MSRV is given by the following
formula:

```text
min(sid, stable - 3)
```

Where `sid` is the current version of `rustc` provided by [Debian Sid], and
`stable` is the latest stable version of Rust. This bound may be broken in case of a major ecosystem shift or a security vulnerability.

[Debian Sid]: https://packages.debian.org/sid/rustc

Orbital is not covered by this MSRV policy, as it requires a Rust nightly
toolchain to compile.

All crates in the [`rust-windowing`] organizations have the
same MSRV policy.

[`rust-windowing`]: https://github.com/rust-windowing

Changelog
---------

See the [changelog](CHANGELOG.md) for a list of this package's versions and the changes made in each version.
