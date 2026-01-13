#[allow(dead_code)]
mod winit_app;

#[allow(unused_imports)]
pub use self::winit_app::*;

/// Initialize an appropriate tracing subscriber, and set the console error hook on WASM.
pub fn setup() {
    #[cfg(not(target_family = "wasm"))]
    {
        use tracing_subscriber::filter::{EnvFilter, LevelFilter};

        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::builder()
                    .with_default_directive(LevelFilter::INFO.into())
                    .from_env_lossy(),
            )
            .init();
    }

    #[cfg(target_family = "wasm")]
    {
        console_error_panic_hook::set_once();

        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .without_time()
                    .with_writer(tracing_web::MakeWebConsoleWriter::new()),
            )
            .init();
    }
}
