//! Example of using `softbuffer` with `libxcb`.

#[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
mod example {
    use raw_window_handle::{
        DisplayHandle, RawDisplayHandle, RawWindowHandle, WindowHandle, XcbDisplayHandle,
        XcbWindowHandle,
    };
    use std::{env, num::NonZeroU32, ptr::NonNull};
    use x11rb::{
        connection::Connection,
        protocol::{
            xproto::{self, ConnectionExt as _},
            Event,
        },
        xcb_ffi::XCBConnection,
    };

    const RED: u32 = 255 << 16;

    pub(crate) fn run() {
        // Create a new XCB connection
        let (conn, screen) = XCBConnection::connect(None).expect("Failed to connect to X server");

        // x11rb doesn't use raw-window-handle yet, so just create our own.
        let display_handle = XcbDisplayHandle::new(
            if env::var_os("SOFTBUFFER_NO_DISPLAY").is_some() {
                None
            } else {
                NonNull::new(conn.get_raw_xcb_connection() as *mut _)
            },
            screen as _,
        );

        // Create a new window.
        let mut width = 640u16;
        let mut height = 480u16;

        let window = conn.generate_id().unwrap();
        let screen = &conn.setup().roots[screen];
        let (root_visual, root_parent) = (screen.root_visual, screen.root);
        conn.create_window(
            x11rb::COPY_FROM_PARENT as _,
            window,
            root_parent,
            0,
            0,
            width,
            height,
            0,
            xproto::WindowClass::COPY_FROM_PARENT,
            root_visual,
            &xproto::CreateWindowAux::new()
                .background_pixel(screen.white_pixel)
                .event_mask(xproto::EventMask::EXPOSURE | xproto::EventMask::STRUCTURE_NOTIFY),
        )
        .unwrap()
        .check()
        .unwrap();

        let mut window_handle = XcbWindowHandle::new(NonZeroU32::new(window).unwrap());
        window_handle.visual_id = NonZeroU32::new(root_visual);

        // Create a new softbuffer context.
        // SAFETY: The display and window handles outlive the context.
        let display_handle =
            unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Xcb(display_handle)) };
        let window_handle =
            unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xcb(window_handle)) };
        let context = softbuffer::Context::new(display_handle).unwrap();
        let mut surface = softbuffer::Surface::new(&context, window_handle).unwrap();

        // Register an atom for closing the window.
        let wm_protocols_atom = conn
            .intern_atom(false, "WM_PROTOCOLS".as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let delete_window_atom = conn
            .intern_atom(false, "WM_DELETE_WINDOW".as_bytes())
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        conn.change_property(
            xproto::PropMode::REPLACE as _,
            window,
            wm_protocols_atom,
            xproto::AtomEnum::ATOM,
            32,
            1,
            &delete_window_atom.to_ne_bytes(),
        )
        .unwrap()
        .check()
        .unwrap();

        // Map the window to the screen.
        conn.map_window(window).unwrap().check().unwrap();

        // Pump events.
        loop {
            let event = conn.wait_for_event().unwrap();

            match event {
                Event::Expose(_) => {
                    // Draw a width x height red rectangle.
                    surface
                        .resize(
                            NonZeroU32::new(width.into()).unwrap(),
                            NonZeroU32::new(height.into()).unwrap(),
                        )
                        .unwrap();
                    let mut buffer = surface.buffer_mut().unwrap();
                    buffer.fill(RED);
                    buffer.present().unwrap();
                }
                Event::ConfigureNotify(configure_notify) => {
                    width = configure_notify.width;
                    height = configure_notify.height;
                }
                Event::ClientMessage(cm) => {
                    if cm.data.as_data32()[0] == delete_window_atom {
                        break;
                    }
                }
                _ => {}
            }
        }

        // Delete the context and drop the window.
        drop(context);
        conn.destroy_window(window).unwrap().check().unwrap();
    }
}

#[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    example::run();
}

#[cfg(not(all(feature = "x11", any(target_os = "linux", target_os = "freebsd"))))]
fn main() {
    eprintln!("This example requires the `x11` feature to be enabled on a supported platform.");
}
