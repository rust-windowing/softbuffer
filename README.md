# Softbuffer

Render an image on the CPU and show it on a window in a cross-platform manner.

There exist many libraries for doing realtime rendering on the GPU, such as `wgpu`, `blade`,
`ash`, etc. This is often the sensible choice, but there are a few cases where it makes sense to
render on the CPU, such as for learning purposes, drawing simple 2D scenes or GUIs, or as a
fallback rendering path when a GPU isn't available. Softbuffer allows you to do this.

To use Softbuffer, first create a window using `winit`, `sdl3`, or any other crate that provides a
[`raw_window_handle::HasWindowHandle`].

Next, you create a [`Context`] and [`Surface`] from that window, and can now call
[`Surface::buffer_mut()`] to get a [`Buffer`] that you can draw into. Once you're done drawing, call
[`Buffer::present()`] to show the buffer on the window.

Note that Softbuffer only provides the `&mut [...]` buffer, it does not provide any rendering
primitives for drawing rectangles, circles, curves and so on. For that, you'll want to use crates
like [`tiny-skia`](https://docs.rs/tiny-skia/) or [`vello_cpu`](https://docs.rs/vello_cpu/).

[`raw_window_handle::HasWindowHandle`]: https://docs.rs/raw-window-handle/0.6.2/raw_window_handle/trait.HasWindowHandle.html

## How it works

Most platforms have a compositor of some sort (WindowServer on macOS, Desktop Window Manager on
Windows, the Wayland compositor, etc). This is a separate process that applications communicate
with over IPC, and it is responsible for taking the various surfaces that applications send to it
and mash ("composite") them together in the right way to render the user's desktop on the
connected monitors.

The role of Softbuffer then is to create a shared memory region (i.e. [`Buffer`]) that can be
written to from the CPU, and then handed to the compositor (in [`Buffer::present`]). Softbuffer
keeps a set of buffers around per surface to implement double-buffering (depending on platform
requirements).

Softbuffer strives to present buffers in a zero-copy manner. One interesting wrinkle here is that
the compositor is often GPU-accelerated, so on platforms without a unified memory architecture,
some copying is inherently necessary (though when possible, it is done in hardware using DMA).

## Platform support

Softbuffer supports many platforms, some to a higher degree than others. This is codified with a "tier" system. Tier 1 platforms can be thought of as "tested and guaranteed to work", tier 2 as "will likely work", and tier 3 as "builds in CI".

The current status is as follows (based on the list of platforms exposed by [`raw-window-handle`](https://crates.io/crates/raw-window-handle)):

|  Platform          | Tier | Available |
| ------------------ | ---- | --------- |
| AppKit (macOS)     | 1    | ✅ |
| Wayland            | 1    | ✅ |
| Win32              | 1    | ✅ |
| XCB / Xlib (X11)   | 1    | ✅ |
| Android NDK        | 2    | ✅ |
| UIKit (iOS)        | 2    | ✅ |
| WebAssembly        | 2    | ✅ |
| DRM/KMS            | 3    | ✅ |
| Orbital            | 3    | ✅ |
| GBM/KMS            | N/A  | ❌ |
| Haiku              | N/A  | ❌ |
| OpenHarmony OS NDK | N/A  | ❌ ([#261](https://github.com/rust-windowing/softbuffer/pull/261)) |
| WinRT              | N/A  | ❌ |
| UEFI               | N/A  | ❌ ([#282](https://github.com/rust-windowing/softbuffer/pull/282)) |

Beware that big endian targets are much less tested, and may behave incorrectly.

Pull requests to add support for new platforms are welcome!

## WebAssembly

To run an example with the web backend: `cargo run-wasm --example winit`

## Android

To run the Android-specific example on an Android phone: `cargo apk r --example winit_android` or `cargo apk r --example winit_multithread_android`.

## Example

```rust,no_run
use std::num::NonZeroU32;
use std::rc::Rc;
use softbuffer::{Context, Pixel, Surface};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

#[path = "../examples/util/mod.rs"]
mod util;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let context = Context::new(event_loop.owned_display_handle()).unwrap();

    let mut app = util::WinitAppBuilder::with_init(
        |elwt| {
            let window = elwt.create_window(Window::default_attributes());
            Rc::new(window.unwrap())
        },
        |_elwt, window| Surface::new(&context, window.clone()).unwrap(),
    )
    .with_event_handler(|window, surface, window_id, event, elwt| {
        elwt.set_control_flow(ControlFlow::Wait);

        if window_id != window.id() {
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                let Some(surface) = surface else {
                    tracing::error!("RedrawRequested fired before Resumed or after Suspended");
                    return;
                };
                let size = window.inner_size();
                surface
                    .resize(
                        NonZeroU32::new(size.width).unwrap(),
                        NonZeroU32::new(size.height).unwrap(),
                    )
                    .unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                for (x, y, pixel) in buffer.pixels_iter() {
                    let red = (x % 255) as u8;
                    let green = (y % 255) as u8;
                    let blue = ((x * y) % 255) as u8;

                    *pixel = Pixel::new_rgb(red, green, blue);
                }

                buffer.present().unwrap();
            }
            WindowEvent::CloseRequested => {
                elwt.exit();
            }
            _ => {}
        }
    });

    event_loop.run_app(&mut app).unwrap();
}
```

## MSRV Policy

This crate's Minimum Supported Rust Version (MSRV) is **1.71**. Changes to
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

## Changelog

See the [changelog](CHANGELOG.md) for a list of this package's versions and the changes made in each version.

## Alternatives

[minifb](https://crates.io/crates/minifb) also allows putting a 2D buffer/image on a window in a platform-independent way.
Minifb's approach to doing window management itself, however, is problematic code duplication. We already have very high quality
libraries for this in the Rust ecosystem (such as [winit](https://crates.io/crates/winit)), and minifb's implementation
of window management is not ideal. For example, it occasionally segfaults and is missing key features such as setting
a window icon on some platforms. While adding these features to minifb would be possible, it makes more sense to use
the standard window handling systems instead.

What about [pixels](https://crates.io/crates/pixels)? Pixels accomplishes a very similar goal to Softbuffer,
however there are two key differences. Pixels provides some capacity for GPU-accelerated post-processing of what is
displayed, while Softbuffer does not. Due to not having this post-processing, Softbuffer does not rely on the GPU or
hardware accelerated graphics stack in any way, and is thus more portable to installations that do not have access to
hardware acceleration (e.g. VMs, older computers, computers with misconfigured drivers). Softbuffer should be used over
pixels when its GPU-accelerated post-processing effects are not needed.

## License & Credits

This library is dual-licensed under MIT or Apache-2.0, just like minifb and rust. Significant portions of code were taken
from the minifb library to do platform-specific work.
