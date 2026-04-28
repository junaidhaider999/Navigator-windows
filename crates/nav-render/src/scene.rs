//! Layout and drawing for overlay “hint pills” (C3: real [`nav_core::Hint`] bounds → client DIPs).
//! Frame classification is NoOp vs full redraw only: partial clips are unsound under
//! `DXGI_SWAP_EFFECT_FLIP_DISCARD` (back-buffer contents undefined after `Present`).

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use nav_core::{Hint, Rect, UiaDebugReject, fallback_anchor_px};
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ROUNDED_RECT, ID2D1Brush, ID2D1DeviceContext,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, IDWriteFactory, IDWriteTextFormat,
};
use windows::core::Interface;

use crate::RenderError;

/// One rounded pill with a UTF-8 label.
#[derive(Clone, Debug, PartialEq)]
pub struct PillGeom {
    pub rect: D2D_RECT_F,
    pub label: String,
    /// Multiplier for pill fill/border/text brush opacity (`1.0` = full strength).
    pub opacity: f32,
    /// When set (debug only): pill center → invoke anchor (truth point) for connector drawing.
    pub debug_connector: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
    /// Resolved invoke anchor in overlay client DIPs (matches [`nav_core::RawHint::anchor_px`] mapping).
    pub anchor_client_dip: D2D_POINT_2F,
    /// Element bounds in client DIPs (UIA bbox mapped into overlay space).
    pub target_bounds_client_dip: D2D_RECT_F,
}

/// Semi-transparent debug rectangle (rejected UIA candidate) in client DIPs.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugRegionGeom {
    pub rect: D2D_RECT_F,
}

/// Whether this frame can skip work or must clear and redraw all pills.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintPlan {
    /// Same pill geometry as last present; skip `BeginDraw` / `Present`.
    NoOp,
    /// Full clear + draw all pills.
    Full,
}

fn duplicate_label(pills: &[PillGeom]) -> bool {
    let mut seen = HashSet::with_capacity(pills.len());
    for p in pills {
        if !seen.insert(p.label.as_str()) {
            return true;
        }
    }
    false
}

fn rects_equal(a: D2D_RECT_F, b: D2D_RECT_F) -> bool {
    (a.left - b.left).abs() <= f32::EPSILON
        && (a.top - b.top).abs() <= f32::EPSILON
        && (a.right - b.right).abs() <= f32::EPSILON
        && (a.bottom - b.bottom).abs() <= f32::EPSILON
}

fn points_equal(a: D2D_POINT_2F, b: D2D_POINT_2F) -> bool {
    (a.x - b.x).abs() <= f32::EPSILON && (a.y - b.y).abs() <= f32::EPSILON
}

fn connector_equal(
    a: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
    b: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
) -> bool {
    match (a, b) {
        (Some((pa0, pa1)), Some((pb0, pb1))) => points_equal(pa0, pb0) && points_equal(pa1, pb1),
        (None, None) => true,
        _ => false,
    }
}

fn opacity_equal(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// Same set of (label, rect) pairs as last frame (order-independent); used to skip identical paints.
pub fn pills_geometrically_equal(a: &[PillGeom], b: &[PillGeom]) -> bool {
    if duplicate_label(a) || duplicate_label(b) {
        return false;
    }
    if a.len() != b.len() {
        return false;
    }
    type V = (
        D2D_RECT_F,
        f32,
        Option<(D2D_POINT_2F, D2D_POINT_2F)>,
        D2D_POINT_2F,
        D2D_RECT_F,
    );
    let mut ma: HashMap<&str, V> = HashMap::with_capacity(a.len());
    for p in a {
        ma.insert(
            p.label.as_str(),
            (
                p.rect,
                p.opacity,
                p.debug_connector,
                p.anchor_client_dip,
                p.target_bounds_client_dip,
            ),
        );
    }
    let mut mb: HashMap<&str, V> = HashMap::with_capacity(b.len());
    for p in b {
        mb.insert(
            p.label.as_str(),
            (
                p.rect,
                p.opacity,
                p.debug_connector,
                p.anchor_client_dip,
                p.target_bounds_client_dip,
            ),
        );
    }
    if ma.len() != mb.len() {
        return false;
    }
    for (k, (r_a, o_a, c_a, aa, ta)) in &ma {
        let Some((r_b, o_b, c_b, ab, tb)) = mb.get(k) else {
            return false;
        };
        if !(rects_equal(*r_a, *r_b)
            && opacity_equal(*o_a, *o_b)
            && connector_equal(*c_a, *c_b)
            && points_equal(*aa, *ab)
            && rects_equal(*ta, *tb))
        {
            return false;
        }
    }
    true
}

fn debug_regions_equal(a: &[DebugRegionGeom], b: &[DebugRegionGeom]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        if !rects_equal(x.rect, y.rect) {
            return false;
        }
    }
    true
}

/// Decide no-op vs full redraw when pills and optional debug regions are unchanged.
///
/// For pill-only frames, pass empty slices for `old_debug` / `new_debug`.
#[must_use]
pub fn overlay_paint_plan(
    old_pills: &[PillGeom],
    new_pills: &[PillGeom],
    old_debug: &[DebugRegionGeom],
    new_debug: &[DebugRegionGeom],
    client_w: f32,
    client_h: f32,
) -> PaintPlan {
    let _ = (client_w, client_h);
    if pills_geometrically_equal(old_pills, new_pills) && debug_regions_equal(old_debug, new_debug)
    {
        PaintPlan::NoOp
    } else {
        PaintPlan::Full
    }
}

/// Em height (DIPs) for hint labels; must match `CreateTextFormat` in `d2d::D2dCompositionRenderer::new`.
pub const PILL_FONT_EM_DIPS: f32 = 11.5;
const PILL_PAD_X: f32 = 4.5;
const PILL_PAD_Y: f32 = 2.5;
/// Tight gap between invoke anchor and pill (4–6 px DIPs; locality over polish).
const PLACE_GAP_DIPS: f32 = 5.0;
/// Minimum inset from overlay edges — pills are translated to fit; never squashed (avoids clipped labels).
const CLIENT_MARGIN_DIPS: f32 = 8.0;
/// Minimum padding when placing inside the target rect.
const INSIDE_INSET_DIPS: f32 = 4.0;

/// Rough average Latin glyph width at [`PILL_FONT_EM_DIPS`] (layout estimate, not shaped text).
const PILL_CHAR_W_EST: f32 = 6.0;
const PILL_MAX_W: f32 = 220.0;

fn estimate_label_width_dips(label: &str) -> f32 {
    let n = label.chars().count().max(1) as f32;
    n * PILL_CHAR_W_EST
}

fn pill_size_for_label(label: &str) -> (f32, f32) {
    let tw = estimate_label_width_dips(label);
    let pw = PILL_PAD_X.mul_add(2.0, tw).min(PILL_MAX_W);
    let ph = PILL_PAD_Y.mul_add(2.0, PILL_FONT_EM_DIPS * 1.06);
    (pw, ph)
}

#[cfg(test)]
fn rects_overlap(a: &D2D_RECT_F, b: &D2D_RECT_F) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

fn rect_intersection_area(a: &D2D_RECT_F, b: &D2D_RECT_F) -> f32 {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    let right = a.right.min(b.right);
    let bottom = a.bottom.min(b.bottom);
    if left < right && top < bottom {
        (right - left) * (bottom - top)
    } else {
        0.0
    }
}

fn overlap_penalty_with_placed(r: &D2D_RECT_F, placed: &[PillGeom]) -> f32 {
    placed
        .iter()
        .map(|p| rect_intersection_area(r, &p.rect))
        .sum()
}

#[derive(Clone, Copy, PartialEq)]
enum PlacementKind {
    Outside,
    Inside,
}

fn pill_center(r: &D2D_RECT_F) -> D2D_POINT_2F {
    D2D_POINT_2F {
        x: (r.left + r.right) * 0.5,
        y: (r.top + r.bottom) * 0.5,
    }
}

fn distance_points(a: D2D_POINT_2F, b: D2D_POINT_2F) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Hard locality budget from physical bbox size (returns DIPs — consistent overlay units).
fn max_anchor_distance_dip(bounds_phys_w: i32, bounds_phys_h: i32) -> f32 {
    let m = bounds_phys_w.max(bounds_phys_h).max(1);
    if m <= 32 {
        24.0
    } else if m <= 96 {
        32.0
    } else {
        40.0
    }
}

fn clamp_anchor_px(x: i32, y: i32, r: &Rect) -> (i32, i32) {
    if r.w <= 0 || r.h <= 0 {
        return (r.x, r.y);
    }
    let max_x = r.x.saturating_add(r.w.saturating_sub(1));
    let max_y = r.y.saturating_add(r.h.saturating_sub(1));
    (x.clamp(r.x, max_x), y.clamp(r.y, max_y))
}

fn resolve_hint_anchor_phys(h: &Hint) -> (i32, i32) {
    let b = h.raw.bounds;
    let (x, y) = h
        .raw
        .anchor_px
        .unwrap_or_else(|| fallback_anchor_px(b, h.raw.kind));
    clamp_anchor_px(x, y, &b)
}

fn phys_to_client_dip(px: i32, py: i32, ox: i32, oy: i32, scale: f32) -> D2D_POINT_2F {
    D2D_POINT_2F {
        x: (px - ox) as f32 * scale,
        y: (py - oy) as f32 * scale,
    }
}

fn rect_fully_inside(inner: &D2D_RECT_F, outer: &D2D_RECT_F) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

/// Translate-only fit inside `[margin, cw-margin] × [margin, ch-margin]` without changing size.
fn fit_rect_in_client_margins(
    left: f32,
    top: f32,
    pw: f32,
    ph: f32,
    cw: f32,
    ch: f32,
    margin: f32,
) -> Option<D2D_RECT_F> {
    if !(pw > 0.0 && ph > 0.0 && cw > 0.0 && ch > 0.0) {
        return None;
    }
    if pw > cw - 2.0 * margin || ph > ch - 2.0 * margin {
        return None;
    }
    let max_l = cw - margin - pw;
    let max_t = ch - margin - ph;
    let left = left.clamp(margin, max_l);
    let top = top.clamp(margin, max_t);
    Some(D2D_RECT_F {
        left,
        top,
        right: left + pw,
        bottom: top + ph,
    })
}

fn inside_anchor(
    el_left: f32,
    el_top: f32,
    el_w: f32,
    el_h: f32,
    pw: f32,
    ph: f32,
) -> Option<(f32, f32)> {
    let inner_w = el_w - 2.0 * INSIDE_INSET_DIPS;
    let inner_h = el_h - 2.0 * INSIDE_INSET_DIPS;
    if inner_w < pw || inner_h < ph {
        return None;
    }
    let left = el_left + INSIDE_INSET_DIPS + (inner_w - pw) * 0.5;
    let top = el_top + INSIDE_INSET_DIPS + (inner_h - ph) * 0.5;
    Some((left, top))
}

/// Anchors around invoke point (TR, TL, BR, BL, R, L, inside bbox).
fn anchor_placement_candidates(
    ax: f32,
    ay: f32,
    pw: f32,
    ph: f32,
    gap: f32,
    target: &D2D_RECT_F,
) -> Vec<(PlacementKind, f32, f32)> {
    let el_left = target.left;
    let el_top = target.top;
    let el_w = target.right - target.left;
    let el_h = target.bottom - target.top;
    let mut v = Vec::with_capacity(8);
    v.push((PlacementKind::Outside, ax + gap, ay - gap - ph));
    v.push((PlacementKind::Outside, ax - gap - pw, ay - gap - ph));
    v.push((PlacementKind::Outside, ax + gap, ay + gap));
    v.push((PlacementKind::Outside, ax - gap - pw, ay + gap));
    v.push((PlacementKind::Outside, ax + gap, ay - ph * 0.5));
    v.push((PlacementKind::Outside, ax - gap - pw, ay - ph * 0.5));
    if let Some((l, t)) = inside_anchor(el_left, el_top, el_w, el_h, pw, ph) {
        v.push((PlacementKind::Inside, l, t));
    }
    v
}

fn local_nudge_offsets() -> &'static [(f32, f32)] {
    &[
        (0.0, 0.0),
        (4.0, 0.0),
        (-4.0, 0.0),
        (0.0, 4.0),
        (0.0, -4.0),
        (4.0, 4.0),
        (-4.0, 4.0),
        (4.0, -4.0),
        (-4.0, -4.0),
        (8.0, 0.0),
        (-8.0, 0.0),
        (0.0, 8.0),
        (0.0, -8.0),
        (8.0, 8.0),
        (-8.0, 8.0),
        (8.0, -8.0),
        (-8.0, -8.0),
        (10.0, 0.0),
        (-10.0, 0.0),
        (0.0, 10.0),
        (0.0, -10.0),
    ]
}

fn score_placement(
    kind: PlacementKind,
    r: &D2D_RECT_F,
    anchor: D2D_POINT_2F,
    target: &D2D_RECT_F,
    placed: &[PillGeom],
    all_anchors: &[D2D_POINT_2F],
    my_idx: usize,
    cw: f32,
    ch: f32,
    max_dist_dip: f32,
) -> f32 {
    let pc = pill_center(r);
    let dist = distance_points(pc, anchor);
    let mut score = dist * 1000.0;

    if dist > max_dist_dip {
        score += (dist - max_dist_dip) * 400.0;
    }

    score += overlap_penalty_with_placed(r, placed) * 4.0;

    let occ = rect_intersection_area(r, target);
    score += match kind {
        PlacementKind::Inside => occ * 0.4,
        PlacementKind::Outside => occ * 12.0,
    };

    for (j, oa) in all_anchors.iter().enumerate() {
        if j == my_idx {
            continue;
        }
        let d_own = distance_points(pc, anchor);
        let d_other = distance_points(pc, *oa);
        if d_other + 1.5 < d_own {
            score += (d_own - d_other) * 600.0;
        }
    }

    let min_edge = (r.left - CLIENT_MARGIN_DIPS)
        .min(r.top - CLIENT_MARGIN_DIPS)
        .min(cw - CLIENT_MARGIN_DIPS - r.right)
        .min(ch - CLIENT_MARGIN_DIPS - r.bottom);
    if min_edge < 3.0 {
        score += (3.0 - min_edge) * 12.0;
    }

    score
}

fn choose_pill_rect(
    anchor: D2D_POINT_2F,
    target: &D2D_RECT_F,
    pw: f32,
    ph: f32,
    placed: &[PillGeom],
    all_anchors: &[D2D_POINT_2F],
    hint_idx: usize,
    client_w: f32,
    client_h: f32,
    max_dist_dip: f32,
) -> Option<D2D_RECT_F> {
    let bases = anchor_placement_candidates(
        anchor.x,
        anchor.y,
        pw,
        ph,
        PLACE_GAP_DIPS,
        target,
    );
    let nudges = local_nudge_offsets();
    let mut best: Option<(f32, D2D_RECT_F)> = None;

    for &(kind, bx, by) in &bases {
        for &(dx, dy) in nudges {
            let left = bx + dx;
            let top = by + dy;
            let Some(r) = fit_rect_in_client_margins(
                left,
                top,
                pw,
                ph,
                client_w,
                client_h,
                CLIENT_MARGIN_DIPS,
            ) else {
                continue;
            };
            if kind == PlacementKind::Inside && !rect_fully_inside(&r, target) {
                continue;
            }
            let s = score_placement(
                kind,
                &r,
                anchor,
                target,
                placed,
                all_anchors,
                hint_idx,
                client_w,
                client_h,
                max_dist_dip,
            );
            best = match best {
                None => Some((s, r)),
                Some((bs, br)) => {
                    if s < bs - 1e-4 {
                        Some((s, r))
                    } else if (s - bs).abs() <= 1e-4 {
                        if distance_points(pill_center(&r), anchor)
                            < distance_points(pill_center(&br), anchor)
                        {
                            Some((s, r))
                        } else {
                            Some((bs, br))
                        }
                    } else {
                        Some((bs, br))
                    }
                }
            };
        }
    }

    best.map(|(_, r)| r)
}

fn clamp_pill_to_client(mut r: D2D_RECT_F, client_w: f32, client_h: f32) -> Option<D2D_RECT_F> {
    const MIN_SPAN: f32 = 4.0;
    r.left = r.left.max(0.0);
    r.top = r.top.max(0.0);
    r.right = r.right.min(client_w);
    r.bottom = r.bottom.min(client_h);
    if r.right - r.left >= MIN_SPAN && r.bottom - r.top >= MIN_SPAN {
        Some(r)
    } else {
        None
    }
}

/// `overlay_origin_phys` is the overlay HWND top-left in physical screen pixels (matches UIA bounds).
/// Maps rejected UIA rows to client-space rectangles (clipped to the overlay client).
pub fn debug_regions_for_frame(
    rejects: &[UiaDebugReject],
    overlay_origin_phys: (i32, i32),
    client_w: f32,
    client_h: f32,
    dpi: f32,
) -> Vec<DebugRegionGeom> {
    let (ox, oy) = overlay_origin_phys;
    let scale = 96.0 / dpi;
    let mut out = Vec::new();
    for r in rejects {
        let Some(b) = r.bounds else {
            continue;
        };
        if b.w <= 0 || b.h <= 0 {
            continue;
        }
        let left = (b.x - ox) as f32 * scale;
        let top = (b.y - oy) as f32 * scale;
        let right = left + b.w as f32 * scale;
        let bottom = top + b.h as f32 * scale;
        let rect = D2D_RECT_F {
            left,
            top,
            right,
            bottom,
        };
        if let Some(clamped) = clamp_pill_to_client(rect, client_w, client_h) {
            out.push(DebugRegionGeom { rect: clamped });
        }
    }
    out
}

/// With several hints visible, soften low planner scores so dense layouts stay readable.
fn pill_opacity_from_cluster(score: f32, max_score: f32, n_hints: usize) -> f32 {
    const MIN_HINTS: usize = 3;
    if n_hints < MIN_HINTS {
        return 1.0;
    }
    if max_score <= 1e-8 || !max_score.is_finite() {
        return 1.0;
    }
    let t = (score / max_score).clamp(0.0, 1.0);
    0.88 + 0.12 * t
}

pub fn pills_for_frame(
    hints: &[Hint],
    overlay_origin_phys: (i32, i32),
    client_w: f32,
    client_h: f32,
    dpi: f32,
    debug_connectors: bool,
) -> Vec<PillGeom> {
    if hints.is_empty() {
        return Vec::new();
    }
    let (ox, oy) = overlay_origin_phys;
    let scale = 96.0 / dpi;
    let anchors_dip: Vec<D2D_POINT_2F> = hints
        .iter()
        .map(|h| {
            let (ax, ay) = resolve_hint_anchor_phys(h);
            phys_to_client_dip(ax, ay, ox, oy, scale)
        })
        .collect();
    let max_score = hints
        .iter()
        .map(|h| h.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_score = if max_score.is_finite() {
        max_score
    } else {
        0.0
    };
    let mut order: Vec<usize> = (0..hints.len()).collect();
    order.sort_by(|&a, &b| {
        hints[b]
            .score
            .partial_cmp(&hints[a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });
    let mut placed = Vec::with_capacity(hints.len());
    for i in order {
        let h = &hints[i];
        let el_left = (h.raw.bounds.x - ox) as f32 * scale;
        let el_top = (h.raw.bounds.y - oy) as f32 * scale;
        let el_w = h.raw.bounds.w as f32 * scale;
        let el_h = h.raw.bounds.h as f32 * scale;
        if el_w <= 0.0 || el_h <= 0.0 || !el_w.is_finite() || !el_h.is_finite() {
            continue;
        }
        let target_bounds = D2D_RECT_F {
            left: el_left,
            top: el_top,
            right: el_left + el_w,
            bottom: el_top + el_h,
        };
        let anchor = anchors_dip[i];
        let max_d = max_anchor_distance_dip(h.raw.bounds.w, h.raw.bounds.h);
        let (pw, ph) = pill_size_for_label(h.label.as_ref());
        if let Some(rect) = choose_pill_rect(
            anchor,
            &target_bounds,
            pw,
            ph,
            &placed,
            &anchors_dip,
            i,
            client_w,
            client_h,
            max_d,
        ) {
            let opacity = pill_opacity_from_cluster(h.score, max_score, hints.len());
            let px = (rect.left + rect.right) * 0.5;
            let py = (rect.top + rect.bottom) * 0.5;
            let debug_connector = if debug_connectors {
                Some((
                    D2D_POINT_2F { x: px, y: py },
                    D2D_POINT_2F {
                        x: anchor.x,
                        y: anchor.y,
                    },
                ))
            } else {
                None
            };
            placed.push(PillGeom {
                rect,
                label: h.label.to_string(),
                opacity,
                debug_connector,
                anchor_client_dip: anchor,
                target_bounds_client_dip: target_bounds,
            });
        }
    }
    placed
}

const CORNER_RADIUS: f32 = 3.5;

#[inline]
unsafe fn solid_brush_set_opacity(
    br: &ID2D1SolidColorBrush,
    opacity: f32,
) -> Result<(), RenderError> {
    let b: ID2D1Brush = br.cast().map_err(|e| RenderError::Win32(e.to_string()))?;
    b.SetOpacity(opacity);
    Ok(())
}

/// Thin line pill → element center (always laid out; opacity scales with `emphasized`).
pub unsafe fn draw_pill_connectors(
    dc: &ID2D1DeviceContext,
    pills: &[PillGeom],
    stroke: &ID2D1StrokeStyle,
    brush: &ID2D1SolidColorBrush,
    emphasized: bool,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    let line_br: ID2D1Brush = brush
        .cast()
        .map_err(|e| RenderError::Win32(e.to_string()))?;
    let width = if emphasized { 1.35 } else { 1.0 };
    let alpha = if emphasized { 0.72 } else { 0.38 };
    for pill in pills {
        let Some((from, to)) = pill.debug_connector else {
            continue;
        };
        solid_brush_set_opacity(brush, alpha)?;
        dc.DrawLine(from, to, &line_br, width, stroke);
        let _ = solid_brush_set_opacity(brush, 1.0);
    }
    Ok(())
}

/// Fills rounded pills and draws centered labels. Call inside `BeginDraw`/`EndDraw`.
/// Fills translucent debug rectangles (drawn under pills).
pub unsafe fn draw_debug_regions(
    dc: &ID2D1DeviceContext,
    regions: &[DebugRegionGeom],
    fill: &ID2D1SolidColorBrush,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    for r in regions {
        dc.FillRectangle(&r.rect, fill);
    }
    Ok(())
}

pub unsafe fn draw_pills(
    dc: &ID2D1DeviceContext,
    text_format: &IDWriteTextFormat,
    write: &IDWriteFactory,
    pills: &[PillGeom],
    fill: &ID2D1SolidColorBrush,
    border: &ID2D1SolidColorBrush,
    text_brush: &ID2D1SolidColorBrush,
    stroke: &ID2D1StrokeStyle,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    let opts = D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;

    for pill in pills {
        solid_brush_set_opacity(fill, pill.opacity)?;
        solid_brush_set_opacity(border, pill.opacity)?;
        solid_brush_set_opacity(text_brush, pill.opacity)?;

        let draw_once = || -> Result<(), RenderError> {
            let rr = D2D1_ROUNDED_RECT {
                rect: pill.rect,
                radiusX: CORNER_RADIUS,
                radiusY: CORNER_RADIUS,
            };
            dc.FillRoundedRectangle(&rr, fill);
            dc.DrawRoundedRectangle(&rr, border, 1.0, stroke);

            let wlabel: Vec<u16> = pill.label.encode_utf16().collect();
            let layout = write
                .CreateTextLayout(
                    &wlabel,
                    text_format,
                    (pill.rect.right - pill.rect.left).max(1.0),
                    (pill.rect.bottom - pill.rect.top).max(1.0),
                )
                .map_err(|e| RenderError::Win32(e.to_string()))?;

            layout
                .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)
                .map_err(|e| RenderError::Win32(e.to_string()))?;
            layout
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)
                .map_err(|e| RenderError::Win32(e.to_string()))?;

            dc.DrawTextLayout(
                D2D_POINT_2F {
                    x: pill.rect.left,
                    y: pill.rect.top,
                },
                &layout,
                text_brush,
                opts,
            );
            Ok(())
        };

        let r = draw_once();
        let _ = solid_brush_set_opacity(fill, 1.0);
        let _ = solid_brush_set_opacity(border, 1.0);
        let _ = solid_brush_set_opacity(text_brush, 1.0);
        r?;
    }
    Ok(())
}

/// Opaque high-contrast pill fill (premultiplied — alpha at full strength for readability).
pub fn pill_fill_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.1,
        g: 0.22,
        b: 0.55,
        a: 0.98,
    }
}

pub fn pill_border_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    }
}

pub fn pill_text_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    }
}

pub fn pill_connector_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.25,
        g: 0.88,
        b: 0.92,
        a: 0.62,
    }
}

pub fn debug_region_fill_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.95,
        g: 0.35,
        b: 0.1,
        a: 0.32,
    }
}

/// Invoke anchor for [`draw_placement_truth`] (red dot).
pub fn placement_truth_dot_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.98,
        g: 0.15,
        b: 0.12,
        a: 0.95,
    }
}

/// Element bounds outline for [`draw_placement_truth`] (green stroke).
pub fn placement_truth_bounds_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.2,
        g: 0.88,
        b: 0.35,
        a: 0.9,
    }
}

/// Optional debug: truth overlay for precision tuning (`config.toml` `[render]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlacementTruthFlags {
    pub target_dot: bool,
    pub target_rect: bool,
    pub distance: bool,
}

/// Red dot = invoke anchor, green outline = element bbox, optional distance (pill center ↔ anchor, DIPs).
pub unsafe fn draw_placement_truth(
    dc: &ID2D1DeviceContext,
    pills: &[PillGeom],
    dot_fill: &ID2D1SolidColorBrush,
    bounds_stroke: &ID2D1SolidColorBrush,
    text_format: &IDWriteTextFormat,
    write: &IDWriteFactory,
    text_brush: &ID2D1SolidColorBrush,
    stroke: &ID2D1StrokeStyle,
    flags: PlacementTruthFlags,
) -> Result<(), RenderError> {
    if !(flags.target_dot || flags.target_rect || flags.distance) {
        return Ok(());
    }
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    let line_br: ID2D1Brush = bounds_stroke
        .cast()
        .map_err(|e| RenderError::Win32(e.to_string()))?;
    let opts = D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;

    for pill in pills {
        if flags.target_rect {
            solid_brush_set_opacity(bounds_stroke, 1.0)?;
            dc.DrawRectangle(&pill.target_bounds_client_dip, &line_br, 1.25, stroke);
        }
        if flags.target_dot {
            solid_brush_set_opacity(dot_fill, 1.0)?;
            let r = 3.0_f32;
            let dot = D2D_RECT_F {
                left: pill.anchor_client_dip.x - r,
                top: pill.anchor_client_dip.y - r,
                right: pill.anchor_client_dip.x + r,
                bottom: pill.anchor_client_dip.y + r,
            };
            dc.FillRectangle(&dot, dot_fill);
        }
        if flags.distance {
            let d = distance_points(pill_center(&pill.rect), pill.anchor_client_dip);
            let label = format!("{d:.0}");
            let wlabel: Vec<u16> = label.encode_utf16().collect();
            let layout = write
                .CreateTextLayout(
                    &wlabel,
                    text_format,
                    48.0,
                    PILL_FONT_EM_DIPS * 1.4,
                )
                .map_err(|e| RenderError::Win32(e.to_string()))?;
            layout
                .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)
                .map_err(|e| RenderError::Win32(e.to_string()))?;
            layout
                .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)
                .map_err(|e| RenderError::Win32(e.to_string()))?;
            solid_brush_set_opacity(text_brush, 0.92)?;
            dc.DrawTextLayout(
                D2D_POINT_2F {
                    x: pill.rect.right + 3.0,
                    y: pill.rect.top,
                },
                &layout,
                text_brush,
                opts,
            );
            let _ = solid_brush_set_opacity(text_brush, 1.0);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_core::{Backend, ElementKind, RawHint, Rect};

    fn hint_at(label: &str, element_id: u64, x: i32, y: i32, w: i32, h: i32) -> Hint {
        hint_scored(label, element_id, 0.0, x, y, w, h)
    }

    fn hint_scored(
        label: &str,
        element_id: u64,
        score: f32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Hint {
        Hint {
            raw: RawHint {
                element_id,
                uia_runtime_id_fp: None,
                uia_invoke_hwnd: None,
                uia_child_index: None,
                bounds: Rect { x, y, w, h },
                anchor_px: None,
                kind: ElementKind::Invoke,
                name: None,
                backend: Backend::Uia,
            },
            label: label.into(),
            score,
        }
    }

    fn any_pairwise_overlap(pills: &[PillGeom]) -> bool {
        for i in 0..pills.len() {
            for j in (i + 1)..pills.len() {
                if rects_overlap(&pills[i].rect, &pills[j].rect) {
                    return true;
                }
            }
        }
        false
    }

    fn pill(label: &str, rect: D2D_RECT_F) -> PillGeom {
        let pc = pill_center(&rect);
        PillGeom {
            rect,
            label: label.into(),
            opacity: 1.0,
            debug_connector: None,
            anchor_client_dip: pc,
            target_bounds_client_dip: rect,
        }
    }

    #[test]
    fn paint_plan_no_op_when_geometrically_identical() {
        let p = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
            },
        )];
        assert_eq!(
            overlay_paint_plan(&p, &p, &[], &[], 100.0, 100.0),
            PaintPlan::NoOp
        );
    }

    #[test]
    fn paint_plan_full_when_one_label_removed_far_apart() {
        let old = vec![
            pill(
                "aa",
                D2D_RECT_F {
                    left: 10.0,
                    top: 10.0,
                    right: 80.0,
                    bottom: 40.0,
                },
            ),
            pill(
                "ab",
                D2D_RECT_F {
                    left: 200.0,
                    top: 10.0,
                    right: 270.0,
                    bottom: 40.0,
                },
            ),
        ];
        let new = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 10.0,
                top: 10.0,
                right: 80.0,
                bottom: 40.0,
            },
        )];
        assert_eq!(
            overlay_paint_plan(&old, &new, &[], &[], 400.0, 300.0),
            PaintPlan::Full
        );
    }

    #[test]
    fn paint_plan_full_when_clearing_all_pills() {
        let old = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
            },
        )];
        let new: Vec<PillGeom> = vec![];
        assert_eq!(
            overlay_paint_plan(&old, &new, &[], &[], 100.0, 100.0),
            PaintPlan::Full
        );
    }

    #[test]
    fn adjacent_toolbar_buttons_avoid_overlap() {
        let hints = vec![
            hint_at("a", 1, 0, 0, 24, 24),
            hint_at("s", 2, 24, 0, 24, 24),
            hint_at("d", 3, 48, 0, 24, 24),
        ];
        let pills = pills_for_frame(&hints, (0, 0), 800.0, 200.0, 96.0, false);
        assert_eq!(pills.len(), 3);
        assert!(
            !any_pairwise_overlap(&pills),
            "expected de-collision, got {:?}",
            pills
        );
    }

    #[test]
    fn higher_score_ranked_first_keeps_both_pills_near_invoke_anchor() {
        let x = 100;
        let y = 100;
        let w = 80;
        let h = 80;
        let low_first = vec![
            hint_scored("low", 1, 0.1, x, y, w, h),
            hint_scored("high", 2, 10.0, x, y, w, h),
        ];
        let pills = pills_for_frame(&low_first, (0, 0), 800.0, 600.0, 96.0, false);
        assert_eq!(pills.len(), 2);
        let dist = |p: &PillGeom| distance_points(pill_center(&p.rect), p.anchor_client_dip);
        let high = pills.iter().find(|p| p.label == "high").expect("high");
        let low = pills.iter().find(|p| p.label == "low").expect("low");
        assert!(
            dist(high) <= 44.0 && dist(low) <= 44.0,
            "expected tight locality to anchor: high={} low={}",
            dist(high),
            dist(low)
        );
    }

    #[test]
    fn cluster_scores_dim_lower_priority_pills() {
        let hints = vec![
            hint_scored("a", 1, 10.0, 0, 0, 28, 28),
            hint_scored("b", 2, 6.0, 30, 0, 28, 28),
            hint_scored("c", 3, 2.0, 60, 0, 28, 28),
            hint_scored("d", 4, 0.5, 90, 0, 28, 28),
        ];
        let pills = pills_for_frame(&hints, (0, 0), 800.0, 200.0, 96.0, false);
        let d = pills.iter().find(|p| p.label == "d").expect("d");
        assert!(
            d.opacity < 0.95,
            "expected dimmed low score in cluster: {}",
            d.opacity
        );
        let a = pills.iter().find(|p| p.label == "a").expect("a");
        assert!(a.opacity > d.opacity, "a={} d={}", a.opacity, d.opacity);
    }

    #[test]
    fn connectors_only_when_debug_enabled() {
        let hints = vec![hint_at("x", 1, 50, 50, 40, 40)];
        let off = pills_for_frame(&hints, (0, 0), 800.0, 600.0, 96.0, false);
        let on = pills_for_frame(&hints, (0, 0), 800.0, 600.0, 96.0, true);
        assert!(off[0].debug_connector.is_none());
        assert!(on[0].debug_connector.is_some());
    }

    #[test]
    fn longer_label_increases_pill_width() {
        let short = pills_for_frame(
            &[hint_at("a", 1, 0, 0, 40, 40)],
            (0, 0),
            800.0,
            600.0,
            96.0,
            false,
        );
        let long = pills_for_frame(
            &[hint_at("abc", 1, 0, 0, 40, 40)],
            (0, 0),
            800.0,
            600.0,
            96.0,
            false,
        );
        assert_eq!(short.len(), 1);
        assert_eq!(long.len(), 1);
        let sw = short[0].rect.right - short[0].rect.left;
        let lw = long[0].rect.right - long[0].rect.left;
        assert!(
            lw > sw + 4.0,
            "long label should widen pill: short={sw} long={lw}"
        );
    }
}
