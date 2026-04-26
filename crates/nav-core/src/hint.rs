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
    pub bounds: Rect,
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
