#![cfg(target_os = "android")]

use winit::event_loop::EventLoopBuilder;
pub use winit::platform::android::{activity::AndroidApp, EventLoopBuilderExtAndroid};

#[path = "winit.rs"]
mod desktop_example;

/// Run with `cargo apk r --example winit_android`
#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut builder = EventLoopBuilder::new();

    // Install the Android event loop extension if necessary.
    builder.with_android_app(app);

    desktop_example::run(builder.build().unwrap())
}
