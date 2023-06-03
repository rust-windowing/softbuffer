use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

#[derive(Debug)]
#[non_exhaustive]
/// A sum type of all of the errors that can occur during the operation of this crate.
pub enum SoftBufferError {
    /// The [`RawDisplayHandle`] passed into [`Context::new`] is not supported by this crate.
    ///
    /// [`RawDisplayHandle`]: raw_window_handle::RawDisplayHandle
    /// [`Context::new`]: crate::Context::new
    UnsupportedDisplayPlatform {
        /// The platform name of the display that was passed into [`Context::new`].
        ///
        /// This is a human-readable string that describes the platform of the display that was
        /// passed into [`Context::new`]. The value is not guaranteed to be stable and this
        /// exists for debugging purposes only.
        ///
        /// [`Context::new`]: crate::Context::new
        human_readable_display_platform_name: &'static str,

        /// The [`RawDisplayHandle`] that was passed into [`Context::new`].
        ///
        /// [`RawDisplayHandle`]: raw_window_handle::RawDisplayHandle
        /// [`Context::new`]: crate::Context::new
        display_handle: RawDisplayHandle,
    },

    /// The [`RawWindowHandle`] passed into [`Surface::new`] is not supported by this crate.
    ///
    /// [`RawWindowHandle`]: raw_window_handle::RawWindowHandle
    /// [`Surface::new`]: crate::Surface::new
    UnsupportedWindowPlatform {
        /// The platform name of the window that was passed into [`Surface::new`].
        ///
        /// This is a human-readable string that describes the platform of the window that was
        /// passed into [`Surface::new`]. The value is not guaranteed to be stable and this
        /// exists for debugging purposes only.
        ///
        /// [`Surface::new`]: crate::Surface::new
        human_readable_window_platform_name: &'static str,

        /// The platform name of the display used by the [`Context`].
        ///
        /// It is possible for a window to be created on a different type of display than the
        /// display that was passed into [`Context::new`]. This is a human-readable string that
        /// describes the platform of the display that was passed into [`Context::new`]. The value
        /// is not guaranteed to be stable and this exists for debugging purposes only.
        ///
        /// [`Context`]: crate::Context
        /// [`Context::new`]: crate::Context::new
        human_readable_display_platform_name: &'static str,

        /// The [`RawWindowHandle`] that was passed into [`Surface::new`].
        ///
        /// [`RawWindowHandle`]: raw_window_handle::RawWindowHandle
        /// [`Surface::new`]: crate::Surface::new
        window_handle: RawWindowHandle,
    },

    /// The [`RawWindowHandle`] passed into [`Surface::new`] is missing necessary fields.
    ///
    /// [`RawWindowHandle`]: raw_window_handle::RawWindowHandle
    /// [`Surface::new`]: crate::Surface::new
    IncompleteWindowHandle,

    /// The [`RawDisplayHandle`] passed into [`Context::new`] is missing necessary fields.
    ///
    /// [`RawDisplayHandle`]: raw_window_handle::RawDisplayHandle
    /// [`Context::new`]: crate::Context::new
    IncompleteDisplayHandle,

    /// The provided size is outside of the range supported by the backend.
    SizeOutOfRange {
        /// The width that was out of range.
        width: NonZeroU32,

        /// The height that was out of range.
        height: NonZeroU32,
    },

    /// The provided damage rect is outside of the range supported by the backend.
    DamageOutOfRange {
        /// The damage rect that was out of range.
        rect: crate::Rect,
    },

    /// A platform-specific backend error occurred.
    ///
    /// The first field provides a human-readable description of the error. The second field
    /// provides the actual error that occurred. Note that the second field is, under the hood,
    /// a private wrapper around the actual error, preventing the user from downcasting to the
    /// actual error type.
    PlatformError(Option<String>, Option<Box<dyn Error>>),

    /// This function is unimplemented on this platform.
    Unimplemented,
}

impl fmt::Display for SoftBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedDisplayPlatform {
                human_readable_display_platform_name,
                display_handle,
            } => write!(
                f,
                "The provided display returned an unsupported platform: {}.\nDisplay handle: {:?}",
                human_readable_display_platform_name, display_handle
            ),
            Self::UnsupportedWindowPlatform {
                human_readable_window_platform_name,
                human_readable_display_platform_name,
                window_handle,
            } => write!(
                f,
                "The provided window returned an unsupported platform: {}, {}.\nWindow handle: {:?}",
                human_readable_window_platform_name, human_readable_display_platform_name, window_handle
            ),
            Self::IncompleteWindowHandle => write!(f, "The provided window handle is null."),
            Self::IncompleteDisplayHandle => write!(f, "The provided display handle is null."),
            Self::SizeOutOfRange { width, height } => write!(
                f,
                "Surface size {width}x{height} out of range for backend.",
            ),
            Self::PlatformError(msg, None) => write!(f, "Platform error: {msg:?}"),
            Self::PlatformError(msg, Some(err)) => write!(f, "Platform error: {msg:?}: {err}"),
            Self::DamageOutOfRange { rect } => write!(
                f,
                "Damage rect {}x{} at ({}, {}) out of range for backend.",
                rect.width, rect.height, rect.x, rect.y
            ),
            Self::Unimplemented => write!(f, "This function is unimplemented on this platform."),
        }
    }
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
