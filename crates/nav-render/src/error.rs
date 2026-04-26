//! Errors from the render worker.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("nav-render is only supported on Windows")]
    UnsupportedPlatform,
    #[error("render worker disconnected")]
    Disconnected,
    #[error("Win32 error: {0}")]
    Win32(String),
    #[error("render thread exited unexpectedly")]
    ThreadExited,
}
