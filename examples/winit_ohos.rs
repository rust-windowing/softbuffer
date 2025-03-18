#![cfg(target_env = "ohos")]

pub use winit::platform::ohos::{ability::OpenHarmonyApp, EventLoopBuilderExtOpenHarmony};
use winit::{event_loop::EventLoop, platform::ohos::ability::ability};

#[path = "winit.rs"]
mod desktop_example;

/// Run with `ohrs build -- --example winit_ohos`
#[ability]
fn openharmony(app: OpenHarmonyApp) {
    let mut builder = EventLoop::builder();

    // Install the Android event loop extension if necessary.
    builder.with_openharmony_app(app);

    desktop_example::entry(builder.build().unwrap())
}
