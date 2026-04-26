//! Errors surfaced by [`crate::UiaRuntime`].

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UiaError {
    #[error("nav-uia is only supported on Windows")]
    UnsupportedPlatform,
    #[error("COM initialization failed (HRESULT 0x{0:08x})")]
    ComInit(i32),
    #[error("failed to create UI Automation instance: {0}")]
    AutomationCreate(String),
    #[error("UI Automation operation failed: {0}")]
    Operation(String),
    #[error("enumeration not supported for this configuration: {0}")]
    UnsupportedConfiguration(&'static str),
    #[error("`invoke` is not implemented in this baseline build")]
    InvokeNotImplemented,
}
