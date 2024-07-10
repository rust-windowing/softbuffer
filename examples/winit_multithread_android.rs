#![cfg(target_os = "android")]

use winit::event_loop::EventLoop;
pub use winit::platform::android::{activity::AndroidApp, EventLoopBuilderExtAndroid};

#[path = "winit_multithread.rs"]
mod desktop_example;

/// Run with `cargo apk r --example winit_android`
#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut builder = EventLoop::builder();

    // Install the Android event loop extension if necessary.
    builder.with_android_app(app);

    desktop_example::ex::entry(builder.build().unwrap())
}
