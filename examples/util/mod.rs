#[allow(dead_code)]
mod winit_app;

#[allow(unused_imports)]
pub use self::winit_app::*;

/// Set up the console error hook on WebAssembly.
pub fn setup() {
    #[cfg(target_family = "wasm")]
    console_error_panic_hook::set_once();
}
