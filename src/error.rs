use std::error::Error;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SwBufError {
    #[error(
        "The provided window returned an unsupported platform: {human_readable_window_platform_name}, {human_readable_display_platform_name}."
    )]
    UnsupportedPlatform {
        human_readable_window_platform_name: &'static str,
        human_readable_display_platform_name: &'static str,
        window_handle: RawWindowHandle,
        display_handle: RawDisplayHandle
    },

    #[error("The provided window handle is null.")]
    IncompleteWindowHandle,

    #[error("The provided display handle is null.")]
    IncompleteDisplayHandle,

    #[error("Platform error")]
    PlatformError(Option<String>, Option<Box<dyn Error>>)
}

#[allow(unused)] // This isn't used on all platforms
pub(crate) fn unwrap<T, E: std::error::Error + 'static>(res: Result<T, E>, str: &str) -> Result<T, SwBufError>{
    match res{
        Ok(t) => Ok(t),
        Err(e) => Err(SwBufError::PlatformError(Some(str.into()), Some(Box::new(e))))
    }
}
