//! Console `tracing` subscriber, enabled only when `--log` is passed.

use std::sync::Once;

static INIT: Once = Once::new();

pub fn init(level: Option<&str>) {
    let Some(level) = level else {
        return;
    };

    INIT.call_once(|| {
        let lev = match level.to_ascii_lowercase().as_str() {
            "trace" => tracing::Level::TRACE,
            "debug" => tracing::Level::DEBUG,
            "info" => tracing::Level::INFO,
            "warn" | "warning" => tracing::Level::WARN,
            "error" => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        };
        tracing_subscriber::fmt()
            .with_max_level(lev)
            .with_target(false)
            .init();
    });
}
