//! Enumeration options (`Agent/workflow/03-modules.md`).

// --- M9 (`10-milestones.md`) default soft stage budgets (ms) ----------------
// These values must match `[fallback.budget_ms]` defaults in `nav-config` and `config.toml` seeds.
/// Soft cap for the UIA / `FindAll` stage in [`FallbackPolicy::Auto`](FallbackPolicy::Auto) (logged when exceeded).
pub const M9_DEFAULT_BUDGET_UIA_MS: u64 = 25;
/// Soft cap for the MSAA / `IAccessible` stage in [`FallbackPolicy::Auto`].
pub const M9_DEFAULT_BUDGET_MSAA_MS: u64 = 8;
/// Soft cap for the raw-`EnumChildWindows` stage in [`FallbackPolicy::Auto`].
pub const M9_DEFAULT_BUDGET_HWND_MS: u64 = 5;
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
            budget_uia_ms: M9_DEFAULT_BUDGET_UIA_MS,
            budget_msaa_ms: M9_DEFAULT_BUDGET_MSAA_MS,
            budget_hwnd_ms: M9_DEFAULT_BUDGET_HWND_MS,
            debug_uia: false,
            debug_overlay: false,
        }
    }
}
