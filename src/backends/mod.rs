#[cfg(target_os = "macos")]
pub(crate) mod cg;
#[cfg(kms_platform)]
pub(crate) mod kms;
#[cfg(target_os = "redox")]
pub(crate) mod orbital;
#[cfg(wayland_platform)]
pub(crate) mod wayland;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(target_os = "windows")]
pub(crate) mod win32;
#[cfg(x11_platform)]
pub(crate) mod x11;
