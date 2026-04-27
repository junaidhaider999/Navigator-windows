//! Pure domain logic for Navigator: hints, labels, filtering, session state.
//!
//! Public contract: `Agent/workflow/03-modules.md`.
//!
//! # Legacy parity
//! Hint label distribution matches Hunt-and-Peck / vimium-style behavior; see [`label`](crate::label).

pub mod error;
pub mod filter;
pub mod geom;
pub mod hint;
pub mod label;
pub mod planner;
pub mod session;
pub mod uia_debug;

pub use error::NavError;
pub use filter::{FilterResult, filter};
pub use geom::Rect;
pub use hint::{Backend, ElementKind, Hint, HintId, RawHint};
pub use label::generate_labels;
pub use planner::plan;
pub use session::{Session, SessionEvent};
pub use uia_debug::{NavEnumerateResult, UiaDebugReject};
