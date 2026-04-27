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

/// Result of a full UIA enumeration pass including optional reject geometry for tooling.
#[derive(Clone, Debug, Default)]
pub struct NavEnumerateResult {
    pub hints: Vec<RawHint>,
    pub debug_rejects: Vec<UiaDebugReject>,
}
