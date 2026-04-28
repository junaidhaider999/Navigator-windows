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

/// UIA `FindAll` tree match: **Fast** = explicit patterns only (low latency). **Full** = also
/// keyboard-focusable + common control types (more candidates, slower on large trees).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum EnumerationProfile {
    #[default]
    Fast,
    Full,
}

/// Per-window enumeration ladder override (`[hints].enumeration_strategy` in config).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EnumerationStrategyMode {
    /// Use [`crate::strategy::probe_window`] + class/exe heuristics.
    #[default]
    Auto,
    /// Always UIA → MSAA → HWND.
    UiaFirst,
    /// HWND (`EnumChildWindows`) → MSAA → UIA (Explorer / Win32-first).
    Win32First,
    /// UIA first but disable Rayon subtree fan-out (Chromium/Electron).
    ChromiumFast,
}

/// Controls what the slow baseline enumerator returns.
#[derive(Clone, Debug)]
pub struct EnumOptions {
    /// Hard cap on returned [`nav_core::RawHint`](nav_core::RawHint) rows (invoke targets).
    pub max_elements: usize,
    pub include_offscreen: bool,
    pub include_disabled: bool,
    pub fallback: FallbackPolicy,
    /// Fast enumerates fewer nodes in the provider; Full keeps parity with older broad matching.
    pub profile: EnumerationProfile,
    /// Stop Rust-side materialization after this wall time (partial hint list is OK).
    pub materialize_hard_budget_ms: u64,
    /// Soft time budgets per enumerator stage (ms). When exceeded, a `tracing::warn` is emitted.
    pub budget_uia_ms: u64,
    pub budget_msaa_ms: u64,
    pub budget_hwnd_ms: u64,
    /// When true, log each skipped UIA node during enumeration to stderr (`[uia-debug]`).
    pub debug_uia: bool,
    /// When true, record skipped nodes with bounds (when known) for a visual debug overlay.
    pub debug_overlay: bool,
    /// See [`EnumerationStrategyMode`].
    pub strategy_mode: EnumerationStrategyMode,
    /// When true, skip Rayon HWND-subtree parallel path in [`crate::enumerate::enumerate_baseline`].
    pub disable_uia_parallel: bool,
    /// When true, run [`TreeScope_Children`] FindAll first (cheap); stop if enough hints (Chromium).
    pub uia_shallow_children_first: bool,
    /// Minimum actionable rows from shallow pass before skipping deep [`TreeScope_Descendants`] FindAll.
    pub uia_shallow_min_targets: usize,
    /// Materialize budget for the shallow pass only (ms).
    pub uia_shallow_materialize_budget_ms: u64,
    /// Explorer Win32-first: if HWND hints count is below this, run a bounded UIA enrich pass (0 = off).
    pub explorer_enrich_if_below: usize,
    /// Materialize budget for Explorer UIA enrich (ms).
    pub explorer_enrich_materialize_budget_ms: u64,
    /// When true, drop UIA hints not intersecting the root HWND client rectangle (screen coords).
    pub clip_uia_to_client_rect: bool,
}

impl Default for EnumOptions {
    fn default() -> Self {
        Self {
            max_elements: 2048,
            include_offscreen: false,
            include_disabled: false,
            fallback: FallbackPolicy::Auto,
            profile: EnumerationProfile::default(),
            materialize_hard_budget_ms: 30,
            budget_uia_ms: M9_DEFAULT_BUDGET_UIA_MS,
            budget_msaa_ms: M9_DEFAULT_BUDGET_MSAA_MS,
            budget_hwnd_ms: M9_DEFAULT_BUDGET_HWND_MS,
            debug_uia: false,
            debug_overlay: false,
            strategy_mode: EnumerationStrategyMode::default(),
            disable_uia_parallel: false,
            uia_shallow_children_first: false,
            uia_shallow_min_targets: 8,
            uia_shallow_materialize_budget_ms: 12,
            explorer_enrich_if_below: 0,
            explorer_enrich_materialize_budget_ms: 28,
            clip_uia_to_client_rect: false,
        }
    }
}
