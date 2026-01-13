//! Example of using softbuffer with drm-rs.

mod util;

#[cfg(all(
    feature = "kms",
    not(any(
        target_os = "android",
        target_vendor = "apple",
        target_os = "redox",
        target_family = "wasm",
        target_os = "windows"
    ))
))]
mod imple {
    use drm::control::{connector, Device as CtrlDevice, Event, ModeTypeFlags, PlaneType};
    use drm::Device;

    use raw_window_handle::{DisplayHandle, DrmDisplayHandle, DrmWindowHandle, WindowHandle};
    use softbuffer::{Context, Surface};

    use std::num::NonZeroU32;
    use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd};
    use std::path::Path;
    use std::time::{Duration, Instant};

    pub(super) fn entry() -> Result<(), Box<dyn std::error::Error>> {
        // Open a new device.
        let device = Card::find()?;

        // Create the softbuffer context.
        let context = unsafe {
            Context::new(DisplayHandle::borrow_raw({
                let handle = DrmDisplayHandle::new(device.as_fd().as_raw_fd());
                handle.into()
            }))
        }?;

        // Get the DRM handles.
        let handles = device.resource_handles()?;

        // Get the list of connectors and CRTCs.
        let connectors = handles
            .connectors()
            .iter()
            .map(|&con| device.get_connector(con, true))
            .collect::<Result<Vec<_>, _>>()?;
        let crtcs = handles
            .crtcs()
            .iter()
            .map(|&crtc| device.get_crtc(crtc))
            .collect::<Result<Vec<_>, _>>()?;

        // Find a connected crtc.
        let con = connectors
            .iter()
            .find(|con| con.state() == connector::State::Connected)
            .ok_or("No connected connectors")?;

        // Get the first CRTC.
        let crtc = crtcs.first().ok_or("No CRTCs")?;

        // Find a mode to use.
        let mode = con
            .modes()
            .iter()
            .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| con.modes().first())
            .ok_or("No modes")?;

        // Look for a primary plane compatible with our CRTC.
        let planes = device.plane_handles()?;
        let planes = planes
            .iter()
            .filter(|&&plane| {
                device.get_plane(plane).is_ok_and(|plane| {
                    let crtcs = handles.filter_crtcs(plane.possible_crtcs());
                    crtcs.contains(&crtc.handle())
                })
            })
            .collect::<Vec<_>>();

        // Find the first primary plane or take the first one period.
        let plane = planes
            .iter()
            .find(|&&&plane| {
                if let Ok(props) = device.get_properties(plane) {
                    let (ids, vals) = props.as_props_and_values();
                    for (&id, &val) in ids.iter().zip(vals.iter()) {
                        if let Ok(info) = device.get_property(id) {
                            if info.name().to_str() == Ok("type") {
                                return val == PlaneType::Primary as u32 as u64;
                            }
                        }
                    }
                }

                false
            })
            .or(planes.first())
            .ok_or("No planes")?;

        // Create the surface on top of this plane.
        // Note: This requires root on DRM/KMS.
        let mut surface = unsafe {
            Surface::new(
                &context,
                WindowHandle::borrow_raw({
                    let handle = DrmWindowHandle::new((**plane).into());
                    handle.into()
                }),
            )
        }?;

        // Resize the surface.
        let (width, height) = mode.size();
        surface.resize(
            NonZeroU32::new(width as u32).unwrap(),
            NonZeroU32::new(height as u32).unwrap(),
        )?;

        // Start drawing to it.
        let start = Instant::now();
        let mut tick = 0;
        while Instant::now().duration_since(start) < Duration::from_secs(2) {
            tick += 1;
            tracing::info!("Drawing tick {tick}");

            // Start drawing.
            let mut buffer = surface.buffer_mut()?;
            draw_to_buffer(&mut buffer, tick);
            buffer.present()?;

            // Wait for the page flip to happen.
            rustix::event::poll(
                &mut [rustix::event::PollFd::new(
                    &device,
                    rustix::event::PollFlags::IN,
                )],
                None,
            )?;

            // Receive the events.
            let events = device.receive_events()?;
            tracing::info!("Got some events...");
            for event in events {
                match event {
                    Event::PageFlip(_) => {
                        tracing::info!("Page flip event.");
                    }
                    Event::Vblank(_) => {
                        tracing::info!("Vblank event.");
                    }
                    _ => {
                        tracing::info!("Unknown event.");
                    }
                }
            }
        }

        Ok(())
    }

    fn draw_to_buffer(buf: &mut [u32], tick: usize) {
        let scale = colorous::SINEBOW;
        let mut i = (tick as f64) / 20.0;
        while i > 1.0 {
            i -= 1.0;
        }

        let color = scale.eval_continuous(i);
        let pixel = ((color.r as u32) << 16) | ((color.g as u32) << 8) | (color.b as u32);
        buf.fill(pixel);
    }

    struct Card(std::fs::File);

    impl Card {
        fn find() -> Result<Card, Box<dyn std::error::Error>> {
            for i in 0..10 {
                let path = format!("/dev/dri/card{i}");
                // Card enumeration may not start at zero, allow failures while opening
                let Ok(device) = Card::open(path) else {
                    continue;
                };

                // Only use it if it has connectors.
                let Ok(handles) = device.resource_handles() else {
                    continue;
                };

                if handles
                    .connectors
                    .iter()
                    .filter_map(|c| device.get_connector(*c, false).ok())
                    .any(|c| c.state() == connector::State::Connected)
                {
                    return Ok(device);
                }
            }

            Err("No DRM device found".into())
        }

        fn open(path: impl AsRef<Path>) -> Result<Card, Box<dyn std::error::Error>> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)?;
            Ok(Card(file))
        }
    }

    impl AsFd for Card {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.0.as_fd()
        }
    }

    impl Device for Card {}
    impl CtrlDevice for Card {}
}

#[cfg(not(all(
    feature = "kms",
    not(any(
        target_os = "android",
        target_vendor = "apple",
        target_os = "redox",
        target_family = "wasm",
        target_os = "windows"
    ))
)))]
mod imple {
    pub(super) fn entry() -> Result<(), Box<dyn std::error::Error>> {
        panic!("This example requires the `kms` feature.")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    util::setup();

    imple::entry()
}
