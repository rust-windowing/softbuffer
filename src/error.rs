use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::error::Error;
use std::fmt;
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

    #[error("This function is unimplemented on this platform")]
    Unimplemented,
}

/// Convenient wrapper to cast errors into SoftBufferError.
pub(crate) trait SwResultExt<T> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError>;
}

impl<T, E: std::error::Error + 'static> SwResultExt<T> for Result<T, E> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError> {
        self.map_err(|e| {
            SoftBufferError::PlatformError(Some(msg.into()), Some(Box::new(LibraryError(e))))
        })
    }
}

impl<T> SwResultExt<T> for Option<T> {
    fn swbuf_err(self, msg: impl Into<String>) -> Result<T, SoftBufferError> {
        self.ok_or_else(|| SoftBufferError::PlatformError(Some(msg.into()), None))
    }
}

/// A wrapper around a library error.
///
/// This prevents `x11-dl` and `x11rb` from becoming public dependencies, since users cannot downcast
/// to this type.
struct LibraryError<E>(E);

impl<E: fmt::Debug> fmt::Debug for LibraryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<E: fmt::Display> fmt::Display for LibraryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for LibraryError<E> {}
