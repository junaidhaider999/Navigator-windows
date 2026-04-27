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
    /// Soft time budgets per enumerator stage (ms). When exceeded, a `tracing::warn` is emitted.
    pub budget_uia_ms: u64,
    pub budget_msaa_ms: u64,
    pub budget_hwnd_ms: u64,
    /// When true, log each skipped UIA node during enumeration to stderr (`[uia-debug]`).
    pub debug_uia: bool,
    /// When true, record skipped nodes with bounds (when known) for a visual debug overlay.
    pub debug_overlay: bool,
}

impl Default for EnumOptions {
    fn default() -> Self {
        Self {
            max_elements: 2048,
            include_offscreen: false,
            include_disabled: false,
            fallback: FallbackPolicy::Auto,
            budget_uia_ms: 25,
            budget_msaa_ms: 8,
            budget_hwnd_ms: 5,
            debug_uia: false,
            debug_overlay: false,
        }
    }
}
