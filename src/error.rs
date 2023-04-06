use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::error::Error;
use std::num::NonZeroU32;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(missing_docs)] // TODO
#[non_exhaustive]
pub enum SoftBufferError {
    #[error(
        "The provided display returned an unsupported platform: {human_readable_display_platform_name}."
    )]
    UnsupportedDisplayPlatform {
        human_readable_display_platform_name: &'static str,
        display_handle: RawDisplayHandle,
    },
    #[error(
        "The provided window returned an unsupported platform: {human_readable_window_platform_name}, {human_readable_display_platform_name}."
    )]
    UnsupportedWindowPlatform {
        human_readable_window_platform_name: &'static str,
        human_readable_display_platform_name: &'static str,
        window_handle: RawWindowHandle,
    },

    #[error("The provided window handle is null.")]
    IncompleteWindowHandle,

    #[error("The provided display handle is null.")]
    IncompleteDisplayHandle,

    #[error("Surface size {width}x{height} out of range for backend.")]
    SizeOutOfRange {
        width: NonZeroU32,
        height: NonZeroU32,
    },

    #[error("Platform error")]
    PlatformError(Option<String>, Option<Box<dyn Error>>),
}

#[allow(unused)] // This isn't used on all platforms
pub(crate) fn unwrap<T, E: std::error::Error + 'static>(
    res: Result<T, E>,
    str: &str,
) -> Result<T, SoftBufferError> {
    match res {
        Ok(t) => Ok(t),
        Err(e) => Err(SoftBufferError::PlatformError(
            Some(str.into()),
            Some(Box::new(e)),
        )),
    }
}
