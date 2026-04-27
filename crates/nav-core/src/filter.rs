//! Prefix filtering over assigned hint labels (`08-hint-generation.md`).

use crate::hint::Hint;

/// Result of narrowing the visible hint set by typed prefix.
#[derive(Debug)]
pub enum FilterResult<'a> {
    /// No hint label starts with the prefix — session should cancel.
    None,
    /// Still ambiguous — render only these hints.
    Many(Vec<&'a Hint>),
    /// Exactly one match — invoke immediately (prefix-free guarantees correctness).
    Single(&'a Hint),
}

/// Keeps hints whose `label` starts with `prefix` (case-sensitive, UTF-8).
///
/// An empty `prefix` matches every hint (initial overlay).
///
/// # Example
///
/// ```
/// use nav_core::{filter, FilterResult, Hint, RawHint, Rect, ElementKind, Backend};
/// let raw = RawHint {
///     element_id: 1,
///     uia_runtime_id_fp: None,
///     uia_invoke_hwnd: None,
///     uia_child_index: None,
///     bounds: Rect { x: 0, y: 0, w: 10, h: 10 },
///     kind: ElementKind::Invoke,
///     name: None,
///     backend: Backend::Uia,
/// };
/// let hints = vec![
///     Hint { raw: raw.clone(), label: "sa".into(), score: 0.0 },
///     Hint { raw, label: "sj".into(), score: 0.0 },
/// ];
/// match filter(&hints, "s") {
///     FilterResult::Many(v) => assert_eq!(v.len(), 2),
///     _ => panic!(),
/// }
/// ```
#[must_use]
pub fn filter<'a>(hints: &'a [Hint], prefix: &str) -> FilterResult<'a> {
    let matches: Vec<&'a Hint> = hints
        .iter()
        .filter(|h| h.label.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => FilterResult::None,
        1 => FilterResult::Single(matches[0]),
        _ => FilterResult::Many(matches),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Backend, ElementKind, RawHint, Rect};

    fn sample_hints() -> Vec<Hint> {
        let raw = RawHint {
            element_id: 0,
            uia_runtime_id_fp: None,
            uia_invoke_hwnd: None,
            uia_child_index: None,
            bounds: Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
            kind: ElementKind::Invoke,
            name: None,
            backend: Backend::Uia,
        };
        vec![
            Hint {
                raw: raw.clone(),
                label: "aa".into(),
                score: 0.0,
            },
            Hint {
                raw,
                label: "ab".into(),
                score: 0.0,
            },
        ]
    }

    #[test]
    fn empty_prefix_is_all_hints() {
        let hints = sample_hints();
        assert!(matches!(filter(&hints, ""), FilterResult::Many(v) if v.len() == 2));
    }

    #[test]
    fn no_starts_with_prefix_is_none() {
        let hints = sample_hints();
        assert!(matches!(filter(&hints, "z"), FilterResult::None));
    }
}
