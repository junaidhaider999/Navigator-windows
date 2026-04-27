//! UIA enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters (D1 cache, D3 parallel).

use core::ffi::c_void;
use std::cell::RefCell;
use std::sync::Arc;

use nav_core::{Backend, RawHint, Rect};
use rayon::prelude::*;
use windows::Win32::Foundation::{HWND, RECT, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationElementArray, TreeScope,
    TreeScope_Children, TreeScope_Descendants,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::core::{BSTR, Error as WinError};

use crate::UiaError;
use crate::cache::{create_enumeration_cache_request, create_invoke_targets_find_condition};
use crate::coords::rect_from_uia_bounds;
use crate::hwnd::UiaHwnd;
use crate::options::EnumOptions;
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
) -> Result<Vec<RawHint>, UiaError> {
    if hwnd.is_invalid() {
        return Ok(Vec::new());
    }

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let (all, root_cached) =
        descendants_cached_or_uncached(&root, TreeScope_Descendants, find_descendants_cond, cache)?;

    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if !root_cached {
        return collect_from_descendants_array(&all, opts, hwnd, None, None, false);
    }

    if len < PARALLEL_DESCENDANT_MIN {
        return collect_from_descendants_array(&all, opts, hwnd, None, None, true);
    }

    let kids = match unsafe { root.FindAllBuildCache(TreeScope_Children, &true_cond, cache) } {
        Ok(k) => k,
        Err(e) if is_pattern_cache_build_failure(&e) => {
            return collect_from_descendants_array(&all, opts, hwnd, None, None, true);
        }
        Err(e) => {
            return Err(UiaError::Operation(format!(
                "FindAllBuildCache Children: {e}"
            )));
        }
    };
    let n_children = unsafe { kids.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if n_children <= 1 {
        return collect_from_descendants_array(&all, opts, hwnd, None, None, true);
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
        return collect_from_descendants_array(&all, opts, hwnd, None, None, true);
    }

    let opts_arc = Arc::new(opts.clone());
    let session_root_bits = hwnd.0 as usize;
    // `HWND` is not `Send` in windows-rs; pass pointer bits for Rayon.
    let hwnd_bits: Vec<usize> = hwnd_subtrees.iter().map(|h| h.0 as usize).collect();
    let parallel: Result<Vec<Vec<RawHint>>, UiaError> = hwnd_bits
        .par_iter()
        .map(|&bits| {
            let sub = HWND(bits as *mut c_void);
            let session_root = HWND(session_root_bits as *mut c_void);
            enumerate_hwnd_subtree_parallel(sub, opts_arc.as_ref(), session_root)
        })
        .collect();

    let mut merged: Vec<RawHint> = match parallel {
        Ok(parts) => parts.into_iter().flatten().collect(),
        Err(_) => return collect_from_descendants_array(&all, opts, hwnd, None, None, true),
    };

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
        )?);
    }

    merged.sort_by(|a, b| {
        a.bounds
            .y
            .cmp(&b.bounds.y)
            .then_with(|| a.bounds.x.cmp(&b.bounds.x))
            .then_with(|| a.element_id.cmp(&b.element_id))
    });
    merged.truncate(opts.max_elements);
    Ok(merged)
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
    cx >= wr.left
        && cx < wr.right
        && cy >= wr.top
        && cy < wr.bottom
}

fn collect_from_descendants_array(
    all: &IUIAutomationElementArray,
    opts: &EnumOptions,
    session_root: HWND,
    scope_hwnd: Option<HWND>,
    child_index: Option<u32>,
    patterns_from_cache: bool,
) -> Result<Vec<RawHint>, UiaError> {
    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
    let mut out = Vec::new();

    for i in 0..len {
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
                    eprintln!(
                        "[uia-debug] skip idx={i} reason=no_interaction name={nm:?}"
                    );
                }
                continue;
            }
        };

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
                    continue;
                }
                Err(_) => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=enabled_err");
                    }
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
                    continue;
                }
                Err(_) => {}
                _ => {}
            }
        }

        let bounds = if patterns_from_cache {
            unsafe { el.CachedBoundingRectangle() }
        } else {
            unsafe { el.CurrentBoundingRectangle() }
        };
        let rect = match bounds {
            Ok(r) => match rect_from_uia_bounds(r) {
                Some(r) => r,
                None => {
                    if opts.debug_uia {
                        eprintln!("[uia-debug] skip idx={i} reason=no_or_zero_rect");
                    }
                    continue;
                }
            },
            Err(_) => {
                if opts.debug_uia {
                    eprintln!("[uia-debug] skip idx={i} reason=bounds_err");
                }
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
            continue;
        }

        let name = read_optional_name(&el, patterns_from_cache);

        out.push(RawHint {
            element_id: i as u64,
            uia_invoke_hwnd: scope_hwnd.map(|h| h.0 as usize),
            uia_child_index: if scope_hwnd.is_none() {
                child_index
            } else {
                None
            },
            bounds: rect,
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
        collect_from_descendants_array(&all, opts, session_root, Some(sub), None, cached)
    })
}
