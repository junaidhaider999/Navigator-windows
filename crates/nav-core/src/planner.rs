//! Rank raw hints and assign vimium-style labels (`08-hint-generation.md`).

use crate::geom::Rect;
use crate::hint::{ElementKind, Hint, RawHint};
use crate::label::{generate_labels, vimium_partition};

const W_SHORT: f32 = 0.0;
const W_PROXIMITY: f32 = 1.0;
const W_KIND: f32 = 0.6;
const W_SIZE: f32 = 0.2;

/// Assigns labels and scores to `raws`. Higher [`Hint::score`] = higher planner priority.
///
/// `max_planned`: keep only the top-N raw hints by [`priority_score`] before labeling (`0` = no cap).
///
/// # Example
///
/// ```
/// use nav_core::{plan, RawHint, Rect, ElementKind, Backend};
/// let raw = RawHint {
///     element_id: 1,
///     uia_runtime_id_fp: None,
///     uia_invoke_hwnd: None,
///     uia_child_index: None,
///     bounds: Rect { x: 0, y: 0, w: 50, h: 20 },
///     anchor_px: None,
///     kind: ElementKind::Invoke,
///     name: None,
///     backend: Backend::Uia,
/// };
/// let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
/// let hints = plan(vec![raw.clone(), raw], &alphabet, Rect { x: 0, y: 0, w: 10, h: 10 }, 0);
/// assert_eq!(hints.len(), 2);
/// ```
#[must_use]
pub fn plan(raws: Vec<RawHint>, alphabet: &[char], layout_origin: Rect, max_planned: usize) -> Vec<Hint> {
    let n_all = raws.len();
    if n_all == 0 {
        return Vec::new();
    }

    let mut scored: Vec<(usize, f32)> = raws
        .iter()
        .enumerate()
        .map(|(i, r)| (i, priority_score(r, layout_origin)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    let cap = if max_planned == 0 {
        n_all
    } else {
        max_planned.min(n_all)
    };

    let picked_raws: Vec<RawHint> = scored
        .iter()
        .take(cap)
        .map(|(i, _)| raws[*i].clone())
        .collect();

    let n = picked_raws.len();
    let labels = generate_labels(n, alphabet);
    let (_digits, long_c, short_c) = vimium_partition(n, alphabet.len());

    let mut label_for: Vec<Option<Box<str>>> = vec![None; n];
    let mut score_for = vec![0.0f32; n];

    for rank in 0..short_c {
        label_for[rank] = Some(labels[long_c + rank].clone());
        score_for[rank] = priority_score(&picked_raws[rank], layout_origin);
    }
    for (k, label) in labels.iter().take(long_c).enumerate() {
        let rank = short_c + k;
        label_for[rank] = Some(label.clone());
        score_for[rank] = priority_score(&picked_raws[rank], layout_origin);
    }

    picked_raws
        .into_iter()
        .enumerate()
        .map(|(i, raw)| Hint {
            raw,
            label: label_for[i].take().expect("label assigned for every index"),
            score: score_for[i],
        })
        .collect()
}

fn kind_weight(k: ElementKind) -> f32 {
    match k {
        ElementKind::Invoke => 1.0,
        ElementKind::Toggle => 0.85,
        ElementKind::Select => 0.8,
        ElementKind::ExpandCollapse => 0.75,
        ElementKind::Editable => 0.5,
        ElementKind::GenericClickable => 0.3,
    }
}

fn priority_score(raw: &RawHint, focus_rect: Rect) -> f32 {
    let _ = W_SHORT;
    let prox = {
        let d = raw.bounds.manhattan_center(focus_rect);
        1.0 / (1.0 + d as f32)
    };
    let kind = W_KIND * kind_weight(raw.kind);
    let area_i = (raw.bounds.w as i64 * raw.bounds.h as i64).max(1);
    let area = area_i as f32;
    let size_term = W_SIZE * (1.0 / area);
    // Penalize huge container rects so short labels go to tighter, likely-real targets.
    const LARGE_PX: i64 = 480_000; // ~800x600
    let large_penalty = if area_i > LARGE_PX { -0.35 } else { 0.0 };

    let cy = raw.bounds.y + raw.bounds.h / 2;
    let rel_y = (cy - focus_rect.y) as f32 / focus_rect.h.max(1) as f32;
    let footer_penalty = if rel_y > 0.88 { -0.12 } else { 0.0 };

    W_PROXIMITY * prox + kind + size_term + large_penalty + footer_penalty
}
