Overview
==
This is a fork of [softbuffer](https://github.com/john01dav/softbuffer) for more active maintenance, as softbuffer now appears to be unmaintained. Currently, it is a drop-in replacement for softbuffer, and many things are the same (including this README).

As the popularity of the library [minifb](https://crates.io/crates/minifb) shows, it is useful to put a 2D buffer/image
on a window in a platform-independent way. Minifb's approach to doing window management itself, however, is problematic
code duplication. We already have very high quality libraries for this in the Rust ecosystem
(such as [winit](https://crates.io/crates/winit)), and minifb's implementation of window management is not ideal. For
example, it occasionally segfaults on some platforms and is missing key features such as the ability to set a window
icon. While it would be possible to add these features to minifb, it makes more sense to instead use the standard
window handling systems.

swbuf integrates with the [raw-window-handle](https://crates.io/crates/raw-window-handle) crate
to allow writing to a window in a cross-platform way while using the very high quality dedicated window management
libraries that are available in the Rust ecosystem.

What about [pixels](https://crates.io/crates/pixels)? Pixels accomplishes a very similar goal to swbuf,
however there are two key differences. Pixels provides some capacity for GPU-accelerated post-processing of what is
displayed, while swbuf does not. Due to not having this post-processing, swbuf does not rely on the GPU or
hardware accelerated graphics stack in any way, and is thus more portable to installations that do not have access to
hardware acceleration (e.g. VMs, older computers, computers with misconfigured drivers). swbuf should be used over
pixels when its GPU-accelerated post-processing effects are not needed.


License & Credits
==

This library is dual-licensed under MIT or Apache-2.0, just like minifb and rust. Significant portions of code were taken
from the minifb library to do platform-specific work.

Platform support:
==
Some, but not all, platforms supported in [raw-window-handle](https://crates.io/crates/raw-window-handle) are supported
by swbuf. Pull requests are welcome to add new platforms! **Nonetheless, all major desktop platforms that winit uses
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
use swbuf::GraphicsContext;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut graphics_context = unsafe { GraphicsContext::new(&window, &window) }.unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::RedrawRequested(window_id) if window_id == window.id() => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };
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

                graphics_context.set_buffer(&buffer, width as u16, height as u16);
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

See git tags for associated commits.

0.1.1
-----
 - Added WASM support (Thanks to [Liamolucko](https://github.com/Liamolucko)!)
 - CALayer is now used for Mac OS backend, which is more flexible about what happens in the windowing library (Thanks to [lunixbochs](https://github.com/lunixbochs)!)

0.1.0
-----
Initial published version with support for Linux (X11 and Wayland), Mac OS (but buggy), and WIndows.
