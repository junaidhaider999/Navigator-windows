//! UI Automation client for Navigator (`Agent/workflow/03-modules.md`, Phase B3 baseline).
//!
//! On Windows this crate performs UIA enumeration using **`FindAllBuildCache`** and a
//! long-lived `IUIAutomationCacheRequest` (Phase D1). Other targets compile stubs so Linux CI stays green.

mod error;
mod hwnd;
pub mod options;

pub use error::UiaError;
pub use hwnd::UiaHwnd;
pub use options::{
    EnumOptions, EnumerationProfile, EnumerationStrategyMode, FallbackPolicy, M9_DEFAULT_BUDGET_HWND_MS,
    M9_DEFAULT_BUDGET_MSAA_MS, M9_DEFAULT_BUDGET_UIA_MS,
};

#[cfg(windows)]
mod cache;
#[cfg(windows)]
mod click;
#[cfg(windows)]
mod coords;
#[cfg(windows)]
mod diagnose;
#[cfg(windows)]
mod enumerate;
#[cfg(windows)]
mod fallback_hwnd;
#[cfg(windows)]
mod fallback_msaa;
#[cfg(windows)]
mod invoke;
#[cfg(windows)]
mod pattern;
#[cfg(windows)]
mod profile;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
mod strategy;

#[cfg(windows)]
pub use runtime::UiaRuntime;
#[cfg(windows)]
pub use strategy::{
    probe_window, resolve_enumeration_behavior, window_cache_key, ResolvedLadder, WindowProbe,
};

#[cfg(not(windows))]
use nav_core::{Hint, NavEnumerateResult};

#[cfg(not(windows))]
/// Stub runtime (all methods error with [`UiaError::UnsupportedPlatform`]).
pub struct UiaRuntime;

#[cfg(not(windows))]
impl UiaRuntime {
    pub fn new() -> Result<Self, UiaError> {
        Err(UiaError::UnsupportedPlatform)
    }

    pub fn enumerate(
        &self,
        _hwnd: UiaHwnd,
        _opts: &EnumOptions,
    ) -> Result<NavEnumerateResult, UiaError> {
        Err(UiaError::UnsupportedPlatform)
    }

    pub fn invoke(
        &self,
        _hwnd: UiaHwnd,
        _hint: &Hint,
        _opts: &EnumOptions,
    ) -> Result<(), UiaError> {
        Err(UiaError::UnsupportedPlatform)
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
