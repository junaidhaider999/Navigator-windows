//! UI Automation client for Navigator (`Agent/workflow/03-modules.md`, Phase B3 baseline).
//!
//! On Windows this crate performs a **slow, uncached** `FindAll` enumeration suitable for
//! regression baselines. Other targets compile stubs so Linux CI stays green.

mod error;
mod hwnd;
pub mod options;

pub use error::UiaError;
pub use hwnd::UiaHwnd;
pub use options::{EnumOptions, FallbackPolicy};

#[cfg(windows)]
mod coords;
#[cfg(windows)]
mod enumerate;
#[cfg(windows)]
mod pattern;
#[cfg(windows)]
mod runtime;

#[cfg(windows)]
pub use runtime::UiaRuntime;

#[cfg(not(windows))]
use nav_core::{Hint, RawHint};

#[cfg(not(windows))]
/// Stub runtime (all methods error with [`UiaError::UnsupportedPlatform`]).
pub struct UiaRuntime;

#[cfg(not(windows))]
impl UiaRuntime {
    pub fn new() -> Result<Self, UiaError> {
        Err(UiaError::UnsupportedPlatform)
    }

    pub fn enumerate(&self, _hwnd: UiaHwnd, _opts: &EnumOptions) -> Result<Vec<RawHint>, UiaError> {
        Err(UiaError::UnsupportedPlatform)
    }

    pub fn invoke(&self, _hint: &Hint) -> Result<(), UiaError> {
        Err(UiaError::InvokeNotImplemented)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    #[test]
    fn new_fails_on_non_windows() {
        assert!(matches!(
            crate::UiaRuntime::new(),
            Err(crate::UiaError::UnsupportedPlatform)
        ));
    }
}
