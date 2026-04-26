//! Reserved for future fallible `nav-core` APIs. Boundary crates (`nav-uia`, `nav-input`, …)
//! own the rich error types today.

use thiserror::Error;

/// Placeholder until `nav-core` exposes fallible operations.
///
/// No `nav-core` API returns `Result<_, NavError>` yet (`03-modules.md` — core helpers are
/// infallible by design). Boundary crates carry real failures.
#[derive(Debug, Clone, Error)]
pub enum NavError {
    #[doc(hidden)]
    #[allow(dead_code)]
    #[error("__reserved — no nav-core errors yet")]
    Reserved,
}
