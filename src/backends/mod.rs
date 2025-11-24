use crate::{ContextInterface, InitError};
use raw_window_handle::HasDisplayHandle;

#[cfg(target_os = "android")]
pub(crate) mod android;
#[cfg(target_vendor = "apple")]
pub(crate) mod cg;
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
pub(crate) mod kms;
#[cfg(target_os = "redox")]
pub(crate) mod orbital;
#[cfg(all(
    feature = "wayland",
    not(any(
        target_os = "android",
        target_vendor = "apple",
        target_os = "redox",
        target_family = "wasm",
        target_os = "windows"
    ))
))]
pub(crate) mod wayland;
#[cfg(target_family = "wasm")]
pub(crate) mod web;
#[cfg(target_os = "windows")]
pub(crate) mod win32;
#[cfg(all(
    feature = "x11",
    not(any(
        target_os = "android",
        target_vendor = "apple",
        target_os = "redox",
        target_family = "wasm",
        target_os = "windows"
    ))
))]
pub(crate) mod x11;

impl<D: HasDisplayHandle> ContextInterface<D> for D {
    fn new(display: D) -> Result<Self, InitError<D>> {
        Ok(display)
    }
}
