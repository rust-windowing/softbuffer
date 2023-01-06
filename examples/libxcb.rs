//! Example of using `softbuffer` with `libxcb`.

#[cfg(all(feature = "x11", any(target_os = "linux", target_os = "freebsd")))]
mod example {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XcbDisplayHandle, XcbWindowHandle};
    use x11rb::{
        connection::Connection,
        protocol::{
            xproto::{self, ConnectionExt as _},
            Event,
        },
        xcb_ffi::XCBConnection,
    };

    pub(crate) fn run() {
        // Create a new XCB connection
        let (conn, screen) = XCBConnection::connect(None).expect("Failed to connect to X server");

        // x11rb doesn't use raw-window-handle yet, so just create our own.
        let mut display_handle = XcbDisplayHandle::empty();
        display_handle.connection = conn.get_raw_xcb_connection() as *mut _;
        display_handle.screen = screen as _;

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

        let mut window_handle = XcbWindowHandle::empty();
        window_handle.window = window as _;
        window_handle.visual_id = root_visual as _;

        // Create a new softbuffer context.
        // SAFETY: The display and window handles outlive the context.
        let context =
            unsafe { softbuffer::Context::from_raw(RawDisplayHandle::Xcb(display_handle)) }
                .unwrap();
        let mut surface =
            unsafe { softbuffer::Surface::from_raw(&context, RawWindowHandle::Xcb(window_handle)) }
                .unwrap();

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
                    let red = 255 << 16;
                    let source = std::iter::repeat(red)
                        .take((width as usize * height as usize) as _)
                        .collect::<Vec<_>>();

                    // Draw the buffer.
                    surface.set_buffer(&source, width, height);
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
