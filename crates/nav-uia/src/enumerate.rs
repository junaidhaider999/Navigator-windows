//! UIA enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters (D1 cache, D3 parallel).

use core::ffi::c_void;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nav_core::{
    Backend, ElementKind, NavEnumerateResult, RawHint, Rect, UiaCoverageStats, UiaDebugReject,
    UiaEnumerateBasis, UiaEnumerateTimingsMs, fnv1a_hash_i32_slice,
};
use rayon::prelude::*;
use windows::Win32::Foundation::{HWND, POINT, RECT, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Ole::{
    SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationElementArray, TreeScope,
    TreeScope_Children, TreeScope_Descendants,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};
use windows::core::{BSTR, Error as WinError};

use crate::UiaError;
use crate::cache::{create_enumeration_cache_request, create_invoke_targets_find_condition};
use crate::coords::rect_from_uia_bounds;
use crate::hwnd::UiaHwnd;
use crate::options::{EnumOptions, EnumerationProfile};
use crate::pattern::classify_interaction_kind;

/// Descendant count at or above which we consider splitting root children across a pool (D3).
const PARALLEL_DESCENDANT_MIN: i32 = 512;
/// Need at least this many distinct native HWND subtrees to pay for Rayon + per-thread COM.
const MIN_PARALLEL_HWND_SUBTREES: usize = 2;

/// Some providers return "Pattern not found" when building a cache that includes Invoke; fall back to `FindAll`.
fn is_pattern_cache_build_failure(err: &WinError) -> bool {
    let s = err.to_string();
    s.contains("Pattern not found")
        || s.contains("0x80040201")
        || s.contains("0x802A0105")
        || s.contains("PATTERNNOTFOUND")
}

fn descendants_cached_or_uncached(
    el: &IUIAutomationElement,
    scope: TreeScope,
    find_cond: &IUIAutomationCondition,
    cache: &IUIAutomationCacheRequest,
) -> Result<(IUIAutomationElementArray, bool), UiaError> {
    match unsafe { el.FindAllBuildCache(scope, find_cond, cache) } {
        Ok(a) => Ok((a, true)),
        Err(e) if is_pattern_cache_build_failure(&e) => {
            let a = unsafe { el.FindAll(scope, find_cond) }.map_err(|e2| {
                UiaError::Operation(format!("FindAllBuildCache: {e}; FindAll fallback: {e2}"))
            })?;
            Ok((a, false))
        }
        Err(e) => Err(UiaError::Operation(format!("FindAllBuildCache: {e}"))),
    }
}

/// Cached enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters.
///
/// `find_descendants_cond` is used for `TreeScope_Descendants` (provider-side pruning). Child
/// lists for HWND splitting still use a true condition so indices match native child order.
pub fn enumerate_baseline(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    opts: &EnumOptions,
    cache: &IUIAutomationCacheRequest,
    find_descendants_cond: &IUIAutomationCondition,
) -> Result<NavEnumerateResult, UiaError> {
    let reject_sink = opts
        .debug_overlay
        .then(|| Arc::new(Mutex::new(Vec::<UiaDebugReject>::new())));

    let finish =
        |hints: Vec<RawHint>, find_ms: f64, mat_ms: f64, coverage: Option<UiaCoverageStats>| {
            NavEnumerateResult {
                hints,
                debug_rejects: take_rejects(&reject_sink),
                timings_ms: Some(UiaEnumerateTimingsMs {
                    findall_ms: find_ms,
                    materialize_ms: mat_ms,
                }),
                coverage,
            }
        };

    if hwnd.is_invalid() {
        return Ok(NavEnumerateResult::default());
    }

    let client_clip = if opts.clip_uia_to_client_rect {
        hwnd_client_screen_rect(hwnd)
    } else {
        None
    };

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    if opts.uia_shallow_children_first {
        let t_shallow_find = Instant::now();
        let (children_arr, ch_cached) = descendants_cached_or_uncached(
            &root,
            TreeScope_Children,
            find_descendants_cond,
            cache,
        )?;
        let shallow_find_ms = t_shallow_find.elapsed().as_secs_f64() * 1000.0;

        let mut shallow_opts = opts.clone();
        shallow_opts.materialize_hard_budget_ms = opts
            .uia_shallow_materialize_budget_ms
            .min(opts.materialize_hard_budget_ms);

        let mut shallow_cov = UiaCoverageStats::default();
        let t_shallow_mat = Instant::now();
        let shallow_hints = collect_from_descendants_array(
            &children_arr,
            &shallow_opts,
            hwnd,
            None,
            None,
            ch_cached,
            &reject_sink,
            UiaEnumerateBasis::RootChildrenOrder,
            Some(&mut shallow_cov),
            client_clip,
        )?;
        let shallow_mat_ms = t_shallow_mat.elapsed().as_secs_f64() * 1000.0;

        if shallow_hints.len() >= opts.uia_shallow_min_targets {
            eprintln!(
                "[uia_shallow] children_only hints={} find_ms={:.2} mat_ms={:.2}",
                shallow_hints.len(),
                shallow_find_ms,
                shallow_mat_ms
            );
            return Ok(NavEnumerateResult {
                hints: shallow_hints,
                debug_rejects: take_rejects(&reject_sink),
                timings_ms: Some(UiaEnumerateTimingsMs {
                    findall_ms: shallow_find_ms,
                    materialize_ms: shallow_mat_ms,
                }),
                coverage: Some(shallow_cov),
            });
        }
        eprintln!(
            "[uia_shallow] fallback_deep shallow_hints={} min_targets={}",
            shallow_hints.len(),
            opts.uia_shallow_min_targets
        );
    }

    let t_find = Instant::now();
    let (all, root_cached) =
        descendants_cached_or_uncached(&root, TreeScope_Descendants, find_descendants_cond, cache)?;
    let find_ms = t_find.elapsed().as_secs_f64() * 1000.0;

    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if !root_cached {
        let mut cov = UiaCoverageStats::default();
        let t_mat = Instant::now();
        let hints = collect_from_descendants_array(
            &all,
            opts,
            hwnd,
            None,
            None,
            false,
            &reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            Some(&mut cov),
            client_clip,
        )?;
        let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
        return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
    }

    if len < PARALLEL_DESCENDANT_MIN || opts.disable_uia_parallel {
        let mut cov = UiaCoverageStats::default();
        let t_mat = Instant::now();
        let hints = collect_from_descendants_array(
            &all,
            opts,
            hwnd,
            None,
            None,
            true,
            &reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            Some(&mut cov),
            client_clip,
        )?;
        let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
        return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
    }

    let kids = match unsafe { root.FindAllBuildCache(TreeScope_Children, &true_cond, cache) } {
        Ok(k) => k,
        Err(e) if is_pattern_cache_build_failure(&e) => {
            let mut cov = UiaCoverageStats::default();
            let t_mat = Instant::now();
            let hints = collect_from_descendants_array(
                &all,
                opts,
                hwnd,
                None,
                None,
                true,
                &reject_sink,
                UiaEnumerateBasis::RootDescendantsOrder,
                Some(&mut cov),
                client_clip,
            )?;
            let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
            return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
        }
        Err(e) => {
            return Err(UiaError::Operation(format!(
                "FindAllBuildCache Children: {e}"
            )));
        }
    };
    let n_children = unsafe { kids.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if n_children <= 1 {
        let mut cov = UiaCoverageStats::default();
        let t_mat = Instant::now();
        let hints = collect_from_descendants_array(
            &all,
            opts,
            hwnd,
            None,
            None,
            true,
            &reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            Some(&mut cov),
            client_clip,
        )?;
        let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
        return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
    }

    let mut hwnd_subtrees: Vec<HWND> = Vec::new();
    let mut no_hwnd_indices: Vec<i32> = Vec::new();

    for j in 0..n_children {
        let el = unsafe { kids.GetElement(j) }.map_err(|e| UiaError::Operation(e.to_string()))?;
        let child_hwnd = unsafe { el.CurrentNativeWindowHandle() }
            .ok()
            .filter(|h| !h.is_invalid() && *h != hwnd);
        match child_hwnd {
            Some(h) => {
                if !hwnd_subtrees.contains(&h) {
                    hwnd_subtrees.push(h);
                }
            }
            None => no_hwnd_indices.push(j),
        }
    }

    hwnd_subtrees.retain(|h| *h != hwnd);

    if hwnd_subtrees.len() < MIN_PARALLEL_HWND_SUBTREES {
        let mut cov = UiaCoverageStats::default();
        let t_mat = Instant::now();
        let hints = collect_from_descendants_array(
            &all,
            opts,
            hwnd,
            None,
            None,
            true,
            &reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            Some(&mut cov),
            client_clip,
        )?;
        let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
        return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
    }

    let opts_arc = Arc::new(opts.clone());
    let reject_arc = reject_sink.clone();
    let session_root_bits = hwnd.0 as usize;
    let hwnd_bits: Vec<usize> = hwnd_subtrees.iter().map(|h| h.0 as usize).collect();
    let parallel: Result<Vec<Vec<RawHint>>, UiaError> = hwnd_bits
        .par_iter()
        .map(|&bits| {
            let sub = HWND(bits as *mut c_void);
            let session_root = HWND(session_root_bits as *mut c_void);
            enumerate_hwnd_subtree_parallel(
                sub,
                opts_arc.as_ref(),
                session_root,
                &reject_arc,
                client_clip,
            )
        })
        .collect();

    let mut merged: Vec<RawHint> = match parallel {
        Ok(parts) => parts.into_iter().flatten().collect(),
        Err(_) => {
            let mut cov = UiaCoverageStats::default();
            let t_mat = Instant::now();
            let hints = collect_from_descendants_array(
                &all,
                opts,
                hwnd,
                None,
                None,
                true,
                &reject_sink,
                UiaEnumerateBasis::RootDescendantsOrder,
                Some(&mut cov),
                client_clip,
            )?;
            let mat_ms = t_mat.elapsed().as_secs_f64() * 1000.0;
            return Ok(finish(hints, find_ms, mat_ms, Some(cov)));
        }
    };

    let t_mat_tail = Instant::now();
    for &j in &no_hwnd_indices {
        let el = unsafe { kids.GetElement(j) }.map_err(|e| UiaError::Operation(e.to_string()))?;
        let (sub, sub_cached) = descendants_cached_or_uncached(
            &el,
            TreeScope_Descendants,
            find_descendants_cond,
            cache,
        )?;
        merged.append(&mut collect_from_descendants_array(
            &sub,
            opts,
            hwnd,
            None,
            Some(j as u32),
            sub_cached,
            &reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            None,
            client_clip,
        )?);
    }
    let mat_tail_ms = t_mat_tail.elapsed().as_secs_f64() * 1000.0;

    merged.sort_by(|a, b| {
        a.bounds
            .y
            .cmp(&b.bounds.y)
            .then_with(|| a.bounds.x.cmp(&b.bounds.x))
            .then_with(|| a.element_id.cmp(&b.element_id))
    });
    merged.truncate(opts.max_elements);
    Ok(finish(merged, find_ms, mat_tail_ms, None))
}

fn take_rejects(sink: &Option<Arc<Mutex<Vec<UiaDebugReject>>>>) -> Vec<UiaDebugReject> {
    sink.as_ref()
        .map(|a| std::mem::take(&mut *a.lock().unwrap()))
        .unwrap_or_default()
}

fn push_reject(
    sink: &Option<Arc<Mutex<Vec<UiaDebugReject>>>>,
    opts: &EnumOptions,
    reason: &str,
    bounds: Option<Rect>,
) {
    if !opts.debug_overlay {
        return;
    }
    let Some(a) = sink else {
        return;
    };
    a.lock().unwrap().push(UiaDebugReject {
        bounds,
        reason: reason.into(),
    });
}

/// FNV-1a hash of UIA `RuntimeId` (stable identity for deduplication).
unsafe fn uia_runtime_id_fingerprint(el: &IUIAutomationElement) -> Option<u64> {
    unsafe {
        let psa = el.GetRuntimeId().ok()?;
        if psa.is_null() {
            return None;
        }
        let out = runtime_id_from_safearray(psa);
        let _ = SafeArrayDestroy(psa);
        out
    }
}

unsafe fn runtime_id_from_safearray(psa: *mut SAFEARRAY) -> Option<u64> {
    unsafe {
        let l = SafeArrayGetLBound(psa, 1).ok()? as i32;
        let u = SafeArrayGetUBound(psa, 1).ok()? as i32;
        if u < l {
            return None;
        }
        let mut parts = Vec::with_capacity((u - l + 1) as usize);
        for idx in l..=u {
            let mut v: i32 = 0;
            SafeArrayGetElement(psa, &idx, &mut v as *mut i32 as *mut c_void).ok()?;
            parts.push(v);
        }
        Some(fnv1a_hash_i32_slice(&parts))
    }
}

fn try_element_bounds(el: &IUIAutomationElement, from_cache: bool) -> Option<Rect> {
    let r = if from_cache {
        unsafe { el.CachedBoundingRectangle() }.ok()?
    } else {
        unsafe { el.CurrentBoundingRectangle() }.ok()?
    };
    rect_from_uia_bounds(r)
}

fn rect_center_inside_hwnd(rect: &Rect, root: HWND) -> bool {
    if root.is_invalid() {
        return true;
    }
    let mut wr = RECT::default();
    if unsafe { GetWindowRect(root, &mut wr) }.is_err() {
        return true;
    }
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    cx >= wr.left && cx < wr.right && cy >= wr.top && cy < wr.bottom
}

fn hwnd_client_screen_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetClientRect(hwnd, &mut r) }.ok()?;
    let mut tl = POINT {
        x: r.left,
        y: r.top,
    };
    let mut br = POINT {
        x: r.right,
        y: r.bottom,
    };
    unsafe {
        if !ClientToScreen(hwnd, &mut tl).as_bool() {
            return None;
        }
        if !ClientToScreen(hwnd, &mut br).as_bool() {
            return None;
        }
    }
    let w = br.x - tl.x;
    let h = br.y - tl.y;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(Rect {
        x: tl.x,
        y: tl.y,
        w,
        h,
    })
}

#[allow(clippy::too_many_arguments)] // UIA enumerate pipe — single hotspot; grouping would obscure flow.
fn collect_from_descendants_array(
    all: &IUIAutomationElementArray,
    opts: &EnumOptions,
    session_root: HWND,
    scope_hwnd: Option<HWND>,
    child_index: Option<u32>,
    patterns_from_cache: bool,
    reject_sink: &Option<Arc<Mutex<Vec<UiaDebugReject>>>>,
    enumerate_basis: UiaEnumerateBasis,
    mut coverage: Option<&mut UiaCoverageStats>,
    client_clip: Option<Rect>,
) -> Result<Vec<RawHint>, UiaError> {
    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
    if let Some(c) = coverage.as_mut() {
        c.raw_nodes = len as usize;
    }
    let mut out = Vec::new();
    let budget = opts.materialize_hard_budget_ms;
    let mat_start = Instant::now();

    for i in 0..len {
        if budget > 0 && mat_start.elapsed() >= Duration::from_millis(budget) {
            break;
        }
        if out.len() >= opts.max_elements {
            break;
        }

        let el = match unsafe { all.GetElement(i) } {
            Ok(e) => e,
            Err(e) => return Err(UiaError::Operation(e.to_string())),
        };

        let kind = match classify_interaction_kind(&el, patterns_from_cache) {
            Some(k) => k,
            None => {
                if opts.debug_uia {
                    let nm = read_optional_name(&el, patterns_from_cache)
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    eprintln!("[uia-debug] skip idx={i} reason=no_interaction name={nm:?}");
                }
                push_reject(
                    reject_sink,
                    opts,
                    "no_interaction",
                    try_element_bounds(&el, patterns_from_cache),
                );
                continue;
            }
        };

        if let Some(c) = coverage.as_mut() {
            c.clickable_candidates += 1;
        }

        if !opts.include_disabled {
            let enabled = if patterns_from_cache {
                unsafe { el.CachedIsEnabled() }
            } else {
                unsafe { el.CurrentIsEnabled() }
            };
            match enabled {
                Ok(b) if !b.as_bool() => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=disabled");
                    }
                    push_reject(
                        reject_sink,
                        opts,
                        "disabled",
                        try_element_bounds(&el, patterns_from_cache),
                    );
                    continue;
                }
                Err(_) => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=enabled_err");
                    }
                    push_reject(
                        reject_sink,
                        opts,
                        "enabled_err",
                        try_element_bounds(&el, patterns_from_cache),
                    );
                    continue;
                }
                _ => {}
            }
        }

        if !opts.include_offscreen {
            let offscreen = if patterns_from_cache {
                unsafe { el.CachedIsOffscreen() }
            } else {
                unsafe { el.CurrentIsOffscreen() }
            };
            match offscreen {
                Ok(b) if b.as_bool() => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=offscreen");
                    }
                    push_reject(
                        reject_sink,
                        opts,
                        "offscreen",
                        try_element_bounds(&el, patterns_from_cache),
                    );
                    continue;
                }
                Err(_) => {}
                _ => {}
            }
        }

        if let Some(c) = coverage.as_mut() {
            c.visible += 1;
        }

        let bounds = if patterns_from_cache {
            unsafe { el.CachedBoundingRectangle() }
        } else {
            unsafe { el.CurrentBoundingRectangle() }
        };
        let rect = match bounds {
            Ok(r) => match rect_from_uia_bounds(r) {
                Some(r) if r.w >= 6 && r.h >= 6 => r,
                Some(_) => {
                    push_reject(reject_sink, opts, "tiny_rect", None);
                    continue;
                }
                None => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=no_or_zero_rect");
                    }
                    push_reject(reject_sink, opts, "no_or_zero_rect", None);
                    continue;
                }
            },
            Err(_) => {
                if opts.debug_uia {
                    eprintln!("[uia-debug] skip idx={i} reason=bounds_err");
                }
                push_reject(reject_sink, opts, "bounds_err", None);
                continue;
            }
        };

        if !rect_center_inside_hwnd(&rect, session_root) {
            if opts.debug_uia {
                eprintln!(
                    "[uia-debug] skip idx={i} reason=outside_root_window bounds=({},{} {}x{})",
                    rect.x, rect.y, rect.w, rect.h
                );
            }
            push_reject(reject_sink, opts, "outside_root_window", Some(rect));
            continue;
        }

        if let Some(clip) = client_clip {
            if !rect.intersects(clip) {
                push_reject(reject_sink, opts, "outside_client_clip", Some(rect));
                continue;
            }
        }

        if let Some(c) = coverage.as_mut() {
            c.after_filter += 1;
        }

        let name = read_optional_name(&el, patterns_from_cache);

        let uia_runtime_id_fp = if opts.profile == EnumerationProfile::Full {
            unsafe { uia_runtime_id_fingerprint(&el) }
        } else {
            None
        };

        if let Some(c) = coverage.as_mut() {
            c.final_hints += 1;
            match kind {
                ElementKind::Invoke => c.kind_invoke += 1,
                ElementKind::Toggle => c.kind_toggle += 1,
                ElementKind::Select => c.kind_select += 1,
                ElementKind::ExpandCollapse => c.kind_expand += 1,
                ElementKind::Editable => c.kind_editable += 1,
                ElementKind::GenericClickable => c.kind_generic += 1,
            }
        }

        out.push(RawHint {
            element_id: i as u64,
            uia_runtime_id_fp,
            uia_invoke_hwnd: scope_hwnd.map(|h| h.0 as usize),
            uia_child_index: if scope_hwnd.is_none() {
                child_index
            } else {
                None
            },
            uia_enumerate_basis: enumerate_basis,
            bounds: rect,
            anchor_px: None,
            kind,
            name,
            backend: Backend::Uia,
        });
    }

    Ok(out)
}

fn read_optional_name(
    el: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    from_cache: bool,
) -> Option<Box<str>> {
    let bstr: BSTR = if from_cache {
        unsafe { el.CachedName() }.ok()?
    } else {
        unsafe { el.CurrentName() }.ok()?
    };
    let s = bstr.to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.into_boxed_str())
    }
}

fn create_uia_instance() -> Result<IUIAutomation, UiaError> {
    unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.or_else(|e8| {
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(|e| {
            UiaError::AutomationCreate(format!("CUIAutomation8: {e8}; CUIAutomation: {e}"))
        })
    })
}

struct ParcelWorker {
    automation: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    co_uninit_on_drop: bool,
}

impl Drop for ParcelWorker {
    fn drop(&mut self) {
        if self.co_uninit_on_drop {
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static PARCEL_WORKER: RefCell<Option<ParcelWorker>> = const { RefCell::new(None) };
}

fn with_parcel_worker<R>(
    f: impl FnOnce(&IUIAutomation, &IUIAutomationCacheRequest) -> Result<R, UiaError>,
) -> Result<R, UiaError> {
    PARCEL_WORKER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            if hr == RPC_E_CHANGED_MODE {
                return Err(UiaError::Operation(
                    "Rayon worker COM mode is incompatible with STA (UIA)".into(),
                ));
            }
            if hr.is_err() {
                return Err(UiaError::ComInit(hr.0));
            }
            let co_uninit_on_drop = hr.0 == 0;
            let automation = create_uia_instance()?;
            let cache = create_enumeration_cache_request(&automation)?;
            *slot = Some(ParcelWorker {
                automation,
                cache,
                co_uninit_on_drop,
            });
        }
        let w = slot.as_ref().unwrap();
        f(&w.automation, &w.cache)
    })
}

fn enumerate_hwnd_subtree_parallel(
    sub: HWND,
    opts: &EnumOptions,
    session_root: HWND,
    reject_sink: &Option<Arc<Mutex<Vec<UiaDebugReject>>>>,
    client_clip: Option<Rect>,
) -> Result<Vec<RawHint>, UiaError> {
    with_parcel_worker(|automation, cache| {
        if sub.is_invalid() {
            return Ok(Vec::new());
        }
        let root = unsafe { automation.ElementFromHandle(sub) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let find_cond = match create_invoke_targets_find_condition(automation, opts) {
            Ok(c) => c,
            Err(_) => unsafe { automation.CreateTrueCondition() }
                .map_err(|e| UiaError::Operation(e.to_string()))?,
        };
        let (all, cached) =
            descendants_cached_or_uncached(&root, TreeScope_Descendants, &find_cond, cache)?;
        collect_from_descendants_array(
            &all,
            opts,
            session_root,
            Some(sub),
            None,
            cached,
            reject_sink,
            UiaEnumerateBasis::RootDescendantsOrder,
            None,
            client_clip,
        )
    })
}
