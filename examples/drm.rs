//! Example of using softbuffer with drm-rs.

#[cfg(kms_platform)]
mod imple {
    use drm::control::{connector, Device as CtrlDevice, ModeTypeFlags, PlaneType};
    use drm::Device;

    use raw_window_handle::{DrmDisplayHandle, DrmWindowHandle};
    use softbuffer::{Context, Surface};

    use std::num::NonZeroU32;
    use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd};
    use std::time::{Duration, Instant};

    pub(super) fn entry() -> Result<(), Box<dyn std::error::Error>> {
        // Open a new device.
        let device = Card::open()?;

        // Create the softbuffer context.
        let context = unsafe {
            Context::from_raw({
                let mut handle = DrmDisplayHandle::empty();
                handle.fd = device.as_fd().as_raw_fd();
                handle.into()
            })
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
                device.get_plane(plane).map_or(false, |plane| {
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
                            if info.name().to_str().map_or(false, |x| x == "type") {
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
            Surface::from_raw(&context, {
                let mut handle = DrmWindowHandle::empty();
                handle.plane = (**plane).into();
                handle.into()
            })
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
            println!("Drawing tick {tick}");

            // Start drawing.
            let mut buffer = surface.buffer_mut()?;
            draw_to_buffer(&mut buffer, tick);
            buffer.present()?;

            // Sleep for a little.
            std::thread::sleep(Duration::from_millis(5));
        }

        Ok(())
    }

    fn draw_to_buffer(buf: &mut [u32], tick: usize) {
        let scale = colorous::VIRIDIS;
        let mut i = (tick as f64) / 10.0;
        while i > 1.0 {
            i -= 1.0;
        }

        let color = scale.eval_continuous(i);
        let pixel = (color.r as u32) << 16 | (color.g as u32) << 8 | (color.b as u32);
        buf.fill(pixel);
    }

    struct Card(std::fs::File);

    impl Card {
        fn open() -> Result<Card, Box<dyn std::error::Error>> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/dri/card0")?;
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

#[cfg(not(kms_platform))]
mod imple {
    pub(super) fn entry() -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("This example requires the `kms` feature.");
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    imple::entry()
}