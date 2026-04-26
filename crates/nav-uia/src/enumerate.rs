//! UIA enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters (D1 cache, D3 parallel).

use core::ffi::c_void;
use std::cell::RefCell;
use std::sync::Arc;

use nav_core::{Backend, ElementKind, RawHint};
use rayon::prelude::*;
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest,
    IUIAutomationElementArray, TreeScope_Children, TreeScope_Descendants,
};
use windows::core::BSTR;

use crate::UiaError;
use crate::cache::create_enumeration_cache_request;
use crate::coords::rect_from_uia_bounds;
use crate::hwnd::UiaHwnd;
use crate::options::EnumOptions;
use crate::pattern::has_invoke_pattern_cached;

/// Descendant count at or above which we consider splitting root children across a pool (D3).
const PARALLEL_DESCENDANT_MIN: i32 = 256;
/// Need at least this many distinct native HWND subtrees to pay for Rayon + per-thread COM.
const MIN_PARALLEL_HWND_SUBTREES: usize = 2;

/// Cached enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters.
pub fn enumerate_baseline(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    opts: &EnumOptions,
    cache: &IUIAutomationCacheRequest,
) -> Result<Vec<RawHint>, UiaError> {
    if hwnd.is_invalid() {
        return Ok(Vec::new());
    }

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let all = unsafe { root.FindAllBuildCache(TreeScope_Descendants, &true_cond, cache) }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache: {e}")))?;

    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if len < PARALLEL_DESCENDANT_MIN {
        return collect_from_descendants_array(&all, opts, None, None);
    }

    let kids = unsafe { root.FindAllBuildCache(TreeScope_Children, &true_cond, cache) }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache Children: {e}")))?;
    let n_children = unsafe { kids.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    if n_children <= 1 {
        return collect_from_descendants_array(&all, opts, None, None);
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
        return collect_from_descendants_array(&all, opts, None, None);
    }

    let opts_arc = Arc::new(opts.clone());
    // `HWND` is not `Send` in windows-rs; pass pointer bits for Rayon.
    let hwnd_bits: Vec<usize> = hwnd_subtrees.iter().map(|h| h.0 as usize).collect();
    let parallel: Result<Vec<Vec<RawHint>>, UiaError> = hwnd_bits
        .par_iter()
        .map(|&bits| {
            let sub = HWND(bits as *mut c_void);
            enumerate_hwnd_subtree_parallel(sub, opts_arc.as_ref())
        })
        .collect();

    let mut merged: Vec<RawHint> = match parallel {
        Ok(parts) => parts.into_iter().flatten().collect(),
        Err(_) => return collect_from_descendants_array(&all, opts, None, None),
    };

    for &j in &no_hwnd_indices {
        let el = unsafe { kids.GetElement(j) }.map_err(|e| UiaError::Operation(e.to_string()))?;
        let sub = unsafe { el.FindAllBuildCache(TreeScope_Descendants, &true_cond, cache) }
            .map_err(|e| UiaError::Operation(format!("subtree FindAllBuildCache: {e}")))?;
        merged.append(&mut collect_from_descendants_array(
            &sub,
            opts,
            None,
            Some(j as u32),
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

fn collect_from_descendants_array(
    all: &IUIAutomationElementArray,
    opts: &EnumOptions,
    scope_hwnd: Option<HWND>,
    child_index: Option<u32>,
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

        if !has_invoke_pattern_cached(&el) {
            continue;
        }

        if !opts.include_disabled {
            match unsafe { el.CurrentIsEnabled() } {
                Ok(b) if !b.as_bool() => continue,
                Err(_) => continue,
                _ => {}
            }
        }

        if !opts.include_offscreen {
            match unsafe { el.CurrentIsOffscreen() } {
                Ok(b) if b.as_bool() => continue,
                Err(_) => {}
                _ => {}
            }
        }

        let rect = match unsafe { el.CurrentBoundingRectangle() } {
            Ok(r) => match rect_from_uia_bounds(r) {
                Some(r) => r,
                None => continue,
            },
            Err(_) => continue,
        };

        let name = read_optional_name(&el);

        out.push(RawHint {
            element_id: i as u64,
            uia_invoke_hwnd: scope_hwnd.map(|h| h.0 as usize),
            uia_child_index: if scope_hwnd.is_none() {
                child_index
            } else {
                None
            },
            bounds: rect,
            kind: ElementKind::Invoke,
            name,
            backend: Backend::Uia,
        });
    }

    Ok(out)
}

fn read_optional_name(
    el: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<Box<str>> {
    let bstr: BSTR = unsafe { el.CurrentName() }.ok()?;
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
) -> Result<Vec<RawHint>, UiaError> {
    with_parcel_worker(|automation, cache| {
        if sub.is_invalid() {
            return Ok(Vec::new());
        }
        let root = unsafe { automation.ElementFromHandle(sub) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let true_cond = unsafe { automation.CreateTrueCondition() }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let all = unsafe { root.FindAllBuildCache(TreeScope_Descendants, &true_cond, cache) }
            .map_err(|e| UiaError::Operation(format!("FindAllBuildCache: {e}")))?;
        collect_from_descendants_array(&all, opts, Some(sub), None)
    })
}
