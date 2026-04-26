//! Enumeration options (`Agent/workflow/03-modules.md`).

/// How to apply MSAA / raw-HWND fallbacks (not used in the B3 baseline path).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FallbackPolicy {
    Auto,
    UiaOnly,
    MsaaOnly,
}

/// Controls what the slow baseline enumerator returns.
#[derive(Clone, Debug)]
pub struct EnumOptions {
    /// Hard cap on returned [`nav_core::RawHint`](nav_core::RawHint) rows (invoke targets).
    pub max_elements: usize,
    pub include_offscreen: bool,
    pub include_disabled: bool,
    pub fallback: FallbackPolicy,
}

impl Default for EnumOptions {
    fn default() -> Self {
        Self {
            max_elements: 1024,
            include_offscreen: false,
            include_disabled: false,
            fallback: FallbackPolicy::Auto,
        }
    }
}
