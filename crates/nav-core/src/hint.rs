//! Domain hint types: raw enumerations from accessibility backends and planned [`Hint`] values.

use crate::geom::Rect;

/// Opaque stable index into the **current** [`Session`](crate::session::Session) hint list
/// (0..`hints.len()` for this session). It is **not** `RawHint::element_id`; orchestration maps
/// `Invoke(HintId)` back to the stored [`Hint`] using this index.
///
/// # Stability
///
/// Valid only until the session is cleared or replaced via [`Session::ingest`](crate::session::Session::ingest).
/// After `ingest`, ids are reassigned in planner order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct HintId(pub u32);

/// One actionable region discovered by an enumerator (UIA, MSAA, or HWND walk).
#[derive(Clone, Debug, PartialEq)]
pub struct RawHint {
    pub element_id: u64,
    /// FNV fingerprint of UIA `RuntimeId` when known; used for deduplication (not used at invoke).
    pub uia_runtime_id_fp: Option<u64>,
    /// When [`Backend::Uia`](Backend::Uia), optional native window used as the root for invoke:
    /// `element_id` indexes `FindAllBuildCache(TreeScope_Descendants)` from this HWND (parallel
    /// subtree enumeration). When `None`, invoke uses the session root HWND from the orchestrator.
    pub uia_invoke_hwnd: Option<usize>,
    /// When [`Backend::Uia`](Backend::Uia) and `uia_invoke_hwnd` is `None`, optional direct-child
    /// index of the session root: invoke resolves `root.FindAllBuildCache(Children).GetElement(j)`
    /// then `FindAllBuildCache(Descendants).GetElement(element_id)`.
    pub uia_child_index: Option<u32>,
    pub bounds: Rect,
    /// Physical-screen point this hint refers to (clickable point when known). If `None`, use
    /// [`crate::anchor::fallback_anchor_px`] with [`bounds`](Self::bounds).
    pub anchor_px: Option<(i32, i32)>,
    pub kind: ElementKind,
    pub name: Option<Box<str>>,
    pub backend: Backend,
}

/// A [`RawHint`] plus assigned label and planner score.
#[derive(Clone, Debug)]
pub struct Hint {
    pub raw: RawHint,
    pub label: Box<str>,
    pub score: f32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Invoke,
    Toggle,
    Select,
    ExpandCollapse,
    Editable,
    GenericClickable,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    Uia,
    Msaa,
    RawHwnd,
}
