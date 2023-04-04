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

✅: Present | ❌: Absent
 - AndroidNdk ❌
 - AppKit ✅ (Thanks to [Seo Sanghyeon](https://github.com/sanxiyn) and [lunixbochs](https://github.com/lunixbochs)!)
 - Orbital ✅
 - UiKit ❌
 - Wayland ✅ (Wayland support in winit is immature at the moment, so it might be wise to force X11 if you're using winit)
 - Web ✅ (Thanks to [Liamolucko](https://github.com/Liamolucko)!)
 - Win32 ✅
 - WinRt ❌
 - Xcb ❌
 - Xlib ✅

Example
==
```rust,no_run
use std::num::NonZeroU32;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let context = unsafe { softbuffer::Context::new(&window) }.unwrap();
    let mut surface = unsafe { softbuffer::Surface::new(&context, &window) }.unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
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
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
```

Changelog
---------

See the [changelog][./CHANGELOG.md] for a list of this package's versions and the changes made in each version.
