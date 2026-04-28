//! Merge redundant accessibility rows before planning — see UX “Phase 1” dedupe rules.

use std::collections::HashMap;

use crate::geom::Rect;
use crate::hint::{ElementKind, RawHint};

/// Counts for `[dedupe] before=… after=… removed=…` logging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DedupeStats {
    pub before: usize,
    pub after: usize,
    pub removed: usize,
}

#[must_use]
pub fn dedupe_raw_hints(candidates: Vec<RawHint>) -> (Vec<RawHint>, DedupeStats) {
    let before = candidates.len();
    if before <= 1 {
        return (
            candidates,
            DedupeStats {
                before,
                after: before,
                removed: 0,
            },
        );
    }

    let mut v = candidates;
    v = dedupe_by_runtime_fp(v);
    v = dedupe_by_bounds_and_name(v);
    v = dedupe_by_center_grid(v);
    v = suppress_fat_parents(v);

    let after = v.len();
    (
        v,
        DedupeStats {
            before,
            after,
            removed: before.saturating_sub(after),
        },
    )
}

fn area(h: &RawHint) -> i64 {
    h.bounds.w as i64 * h.bounds.h as i64
}

fn kind_priority(k: ElementKind) -> u8 {
    match k {
        ElementKind::Invoke => 6,
        ElementKind::Toggle => 5,
        ElementKind::Select => 5,
        ElementKind::ExpandCollapse => 4,
        ElementKind::Editable => 3,
        ElementKind::GenericClickable => 2,
    }
}

/// True if `a` is strictly better to keep than `b` (same logical target disambiguation).
fn keep_a_over_b(a: &RawHint, b: &RawHint) -> bool {
    match kind_priority(a.kind).cmp(&kind_priority(b.kind)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            let aa = area(a);
            let ab = area(b);
            match aa.cmp(&ab) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => a.element_id <= b.element_id,
            }
        }
    }
}

fn dedupe_by_runtime_fp(candidates: Vec<RawHint>) -> Vec<RawHint> {
    let mut best: HashMap<u64, RawHint> = HashMap::new();
    let mut rest: Vec<RawHint> = Vec::new();
    for h in candidates {
        match h.uia_runtime_id_fp {
            Some(fp) => {
                best.entry(fp)
                    .and_modify(|e| {
                        if keep_a_over_b(&h, e) {
                            *e = h.clone();
                        }
                    })
                    .or_insert(h);
            }
            None => rest.push(h),
        }
    }
    rest.extend(best.into_values());
    rest
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
struct BoundsNameKey(i32, i32, i32, i32, u64);

fn name_fp(n: &Option<Box<str>>) -> u64 {
    let s = n.as_deref().unwrap_or("").trim();
    let mut h: u64 = 14695981039346656037;
    for b in s.to_lowercase().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn dedupe_by_bounds_and_name(candidates: Vec<RawHint>) -> Vec<RawHint> {
    let mut best: HashMap<BoundsNameKey, RawHint> = HashMap::new();
    for h in candidates {
        let k = BoundsNameKey(
            h.bounds.x,
            h.bounds.y,
            h.bounds.w,
            h.bounds.h,
            name_fp(&h.name),
        );
        best.entry(k)
            .and_modify(|e| {
                if keep_a_over_b(&h, e) {
                    *e = h.clone();
                }
            })
            .or_insert(h);
    }
    best.into_values().collect()
}

fn center(b: &Rect) -> (i32, i32) {
    (b.x + b.w / 2, b.y + b.h / 2)
}

const CENTER_QUANT: i32 = 4;

fn center_cell(b: &Rect) -> (i32, i32) {
    let (cx, cy) = center(b);
    (cx.div_euclid(CENTER_QUANT), cy.div_euclid(CENTER_QUANT))
}

fn dedupe_by_center_grid(candidates: Vec<RawHint>) -> Vec<RawHint> {
    let mut best: HashMap<(i32, i32), RawHint> = HashMap::new();
    for h in candidates {
        let key = center_cell(&h.bounds);
        best.entry(key)
            .and_modify(|e| {
                if keep_a_over_b(&h, e) {
                    *e = h.clone();
                }
            })
            .or_insert(h);
    }
    best.into_values().collect()
}

fn rect_contains_point(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Prefer small, inner rects: drop larger parents whose **center** is already covered by a kept smaller element.
fn suppress_fat_parents(candidates: Vec<RawHint>) -> Vec<RawHint> {
    let mut sorted = candidates;
    sorted.sort_by_key(area);
    let mut kept: Vec<RawHint> = Vec::with_capacity(sorted.len());
    'next: for h in sorted {
        let ah = area(&h);
        if ah <= 0 {
            continue;
        }
        let (cx, cy) = center(&h.bounds);
        for k in &kept {
            let ak = area(k);
            if ak >= ah {
                continue;
            }
            if rect_contains_point(k.bounds, cx, cy) {
                continue 'next;
            }
        }
        kept.push(h);
    }
    kept
}

/// Hash UIA runtime id parts for `RawHint::uia_runtime_id_fp` (nav-uia only).
#[must_use]
pub fn fnv1a_hash_i32_slice(parts: &[i32]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for p in parts {
        for b in p.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(1099511628211);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hint::Backend;

    #[allow(clippy::too_many_arguments)]
    fn h(
        id: u64,
        x: i32,
        y: i32,
        w: i32,
        h_: i32,
        kind: ElementKind,
        fp: Option<u64>,
        name: Option<&str>,
    ) -> RawHint {
        RawHint {
            element_id: id,
            uia_runtime_id_fp: fp,
            uia_invoke_hwnd: None,
            uia_child_index: None,
            bounds: Rect { x, y, w, h: h_ },
            anchor_px: None,
            kind,
            name: name.map(|s| s.into()),
            backend: Backend::Uia,
        }
    }

    #[test]
    fn duplicate_runtime_fp_keeps_smaller_invoke() {
        let a = h(
            0,
            0,
            0,
            100,
            100,
            ElementKind::GenericClickable,
            Some(42),
            None,
        );
        let b = h(1, 10, 10, 20, 20, ElementKind::Invoke, Some(42), None);
        let (out, st) = dedupe_raw_hints(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(st.removed, 1);
        assert_eq!(out[0].kind, ElementKind::Invoke);
    }

    #[test]
    fn center_grid_merges_overlapping_targets() {
        let a = h(0, 0, 0, 10, 10, ElementKind::Invoke, None, Some("x"));
        let b = h(
            1,
            2,
            2,
            6,
            6,
            ElementKind::GenericClickable,
            None,
            Some("y"),
        );
        let (out, _) = dedupe_raw_hints(vec![a.clone(), b.clone()]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn parent_suppressed_when_child_claims_center() {
        let parent = h(0, 0, 0, 200, 200, ElementKind::Invoke, None, None);
        let child = h(1, 80, 80, 40, 40, ElementKind::Invoke, None, None);
        let (out, st) = dedupe_raw_hints(vec![parent, child.clone()]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].element_id, 1);
        assert!(st.removed >= 1);
    }
}
