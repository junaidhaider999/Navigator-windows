//! Invoke pattern dispatch: re-resolve the same `FindAllBuildCache` index as enumeration (D1).
//!
//! Enumeration uses `AutomationElementMode_None` for speed. Invoke repeats `FindAllBuildCache`
//! with a **Full**-mode cache (`create_invoke_findall_cache_request`) so `GetElement` yields a
//! real element for `GetCachedPattern` / `GetCurrentPattern` / `Invoke`.

use core::ffi::c_void;

use nav_core::Hint;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationInvokePattern, TreeScope_Children, TreeScope_Descendants, UIA_InvokePatternId,
};
use windows::core::Interface;

use crate::UiaError;
use crate::hwnd::UiaHwnd;

/// Invokes the element identified by [`Hint::raw`](nav_core::RawHint) (`element_id` + optional
/// `uia_invoke_hwnd` / `uia_child_index` scoping).
pub fn invoke_invoke_pattern(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    hint: &Hint,
    find_cache: &IUIAutomationCacheRequest,
    find_descendants_cond: &IUIAutomationCondition,
) -> Result<(), UiaError> {
    if hwnd.is_invalid() {
        return Err(UiaError::Operation("invalid HWND for invoke".into()));
    }

    let idx = hint.raw.element_id;
    if idx > i32::MAX as u64 {
        return Err(UiaError::Operation("element_id out of range".into()));
    }
    let idx = idx as i32;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let el: IUIAutomationElement = if let Some(mem) = hint.raw.uia_invoke_hwnd {
        let base = HWND(mem as *mut c_void);
        let root = unsafe { automation.ElementFromHandle(base) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let all = unsafe {
            root.FindAllBuildCache(TreeScope_Descendants, find_descendants_cond, find_cache)
        }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache (scoped hwnd): {e}")))?;
        let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        bounds_check(idx, len)?;
        unsafe { all.GetElement(idx) }.map_err(|e| UiaError::Operation(e.to_string()))?
    } else if let Some(ci) = hint.raw.uia_child_index {
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let kids = unsafe { root.FindAllBuildCache(TreeScope_Children, &true_cond, find_cache) }
            .map_err(|e| UiaError::Operation(format!("FindAllBuildCache Children: {e}")))?;
        let c = unsafe { kids.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        if ci as i32 >= c {
            return Err(UiaError::Operation(format!(
                "uia_child_index {ci} out of bounds (children len={c})"
            )));
        }
        let subroot = unsafe { kids.GetElement(ci as i32) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let all = unsafe {
            subroot.FindAllBuildCache(TreeScope_Descendants, find_descendants_cond, find_cache)
        }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache subtree: {e}")))?;
        let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        bounds_check(idx, len)?;
        unsafe { all.GetElement(idx) }.map_err(|e| UiaError::Operation(e.to_string()))?
    } else {
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let all = unsafe {
            root.FindAllBuildCache(TreeScope_Descendants, find_descendants_cond, find_cache)
        }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache: {e}")))?;
        let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        bounds_check(idx, len)?;
        unsafe { all.GetElement(idx) }.map_err(|e| UiaError::Operation(e.to_string()))?
    };

    let pat = match unsafe { el.GetCachedPattern(UIA_InvokePatternId) } {
        Ok(p) => p,
        Err(e1) => unsafe { el.GetCurrentPattern(UIA_InvokePatternId) }.map_err(|e2| {
            UiaError::Operation(format!(
                "Invoke pattern GetCachedPattern: {e1}; GetCurrentPattern: {e2}"
            ))
        })?,
    };
    let invoke: IUIAutomationInvokePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { invoke.Invoke() }.map_err(|e| UiaError::Operation(e.to_string()))?;
    Ok(())
}

fn bounds_check(idx: i32, len: i32) -> Result<(), UiaError> {
    if idx < 0 || idx >= len {
        return Err(UiaError::Operation(format!(
            "invoke index {idx} out of bounds (len={len})"
        )));
    }
    Ok(())
}
