use std::error::Error;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SoftBufferError<W: HasRawWindowHandle> {
    #[error(
        "The provided window returned an unsupported platform: {human_readable_platform_name}."
    )]
    UnsupportedPlatform {
        window: W,
        human_readable_platform_name: &'static str,
        handle: RawWindowHandle,
    },
    #[error("Platform error")]
    PlatformError(Option<String>, Option<Box<dyn Error>>)
}
