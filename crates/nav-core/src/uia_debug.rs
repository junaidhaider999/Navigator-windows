//! Optional diagnostics from UI Automation enumeration (debug overlay).

use crate::geom::Rect;
use crate::hint::RawHint;

/// A node that matched the provider `FindAll` filter but was dropped during collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiaDebugReject {
    /// Bounding rect when known (physical screen pixels); `None` if bounds were unavailable.
    pub bounds: Option<Rect>,
    /// Short machine reason (e.g. `no_interaction`, `disabled`, `outside_root_window`).
    pub reason: Box<str>,
}

/// Split timings for UIA `FindAllBuildCache` vs post-fetch materialization (Rust-side loop).
#[derive(Clone, Copy, Debug, Default)]
pub struct UiaEnumerateTimingsMs {
    pub findall_ms: f64,
    pub materialize_ms: f64,
}

/// Pipeline counters for stderr diagnostics (`[coverage]` / `[profile_stats]`).
#[derive(Clone, Copy, Debug, Default)]
pub struct UiaCoverageStats {
    pub raw_nodes: usize,
    pub clickable_candidates: usize,
    /// Passed enabled + UIA offscreen gates (still on-screen per provider flag).
    pub visible: usize,
    /// Passed geom filters (min size, HWND center, optional client-area clip).
    pub after_filter: usize,
    pub final_hints: usize,
    pub kind_invoke: usize,
    pub kind_toggle: usize,
    pub kind_select: usize,
    pub kind_expand: usize,
    pub kind_editable: usize,
    pub kind_generic: usize,
}

/// Result of a full UIA enumeration pass including optional reject geometry for tooling.
#[derive(Clone, Debug, Default)]
pub struct NavEnumerateResult {
    pub hints: Vec<RawHint>,
    pub debug_rejects: Vec<UiaDebugReject>,
    pub timings_ms: Option<UiaEnumerateTimingsMs>,
    pub coverage: Option<UiaCoverageStats>,
}
