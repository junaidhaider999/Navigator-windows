//! UIA dispatch: re-resolve the same `FindAllBuildCache` index as enumeration, then invoke pattern,
//! toggle, selection, expand/collapse, legacy default action, or physical click fallback.

use core::ffi::c_void;

use nav_core::{ElementKind, Hint};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    ExpandCollapseState_Collapsed, ExpandCollapseState_LeafNode,
    ExpandCollapseState_PartiallyExpanded, IUIAutomation,
    IUIAutomationCacheRequest, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, TreeScope_Children,
    TreeScope_Descendants, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionItemPatternId, UIA_TogglePatternId,
};
use windows::core::Interface;

use crate::UiaError;
use crate::click::left_click_rect_center;
use crate::hwnd::UiaHwnd;

/// Resolves the enumerated element and performs the interaction implied by [`Hint::raw.kind`](nav_core::RawHint).
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

    dispatch_on_element(&el, hint)
}

fn bounds_check(idx: i32, len: i32) -> Result<(), UiaError> {
    if idx < 0 || idx >= len {
        return Err(UiaError::Operation(format!(
            "invoke index {idx} out of bounds (len={len})"
        )));
    }
    Ok(())
}

fn pattern_cached_or_current(
    el: &IUIAutomationElement,
    id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
) -> Result<windows::core::IUnknown, UiaError> {
    match unsafe { el.GetCachedPattern(id) } {
        Ok(p) => Ok(p),
        Err(e1) => unsafe { el.GetCurrentPattern(id) }.map_err(|e2| {
            UiaError::Operation(format!(
                "GetCachedPattern: {e1}; GetCurrentPattern: {e2}"
            ))
        }),
    }
}

fn dispatch_on_element(el: &IUIAutomationElement, hint: &Hint) -> Result<(), UiaError> {
    match hint.raw.kind {
        ElementKind::Invoke => {
            let pat = pattern_cached_or_current(el, UIA_InvokePatternId)?;
            let invoke: IUIAutomationInvokePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { invoke.Invoke() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        }
        ElementKind::Toggle => {
            let pat = pattern_cached_or_current(el, UIA_TogglePatternId)?;
            let p: IUIAutomationTogglePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Toggle() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        }
        ElementKind::Select => {
            let pat = pattern_cached_or_current(el, UIA_SelectionItemPatternId)?;
            let p: IUIAutomationSelectionItemPattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Select() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        }
        ElementKind::ExpandCollapse => {
            let pat = pattern_cached_or_current(el, UIA_ExpandCollapsePatternId)?;
            let p: IUIAutomationExpandCollapsePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            let state = unsafe { p.CachedExpandCollapseState() }
                .or_else(|_| unsafe { p.CurrentExpandCollapseState() })
                .map_err(|e| UiaError::Operation(e.to_string()))?;
            if state == ExpandCollapseState_LeafNode {
                left_click_rect_center(&hint.raw.bounds)?;
            } else if state == ExpandCollapseState_Collapsed
                || state == ExpandCollapseState_PartiallyExpanded
            {
                unsafe { p.Expand() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            } else {
                unsafe { p.Collapse() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            }
        }
        ElementKind::GenericClickable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        return Ok(());
                    }
                }
            }
            left_click_rect_center(&hint.raw.bounds)?;
        }
        ElementKind::Editable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        return Ok(());
                    }
                }
            }
            left_click_rect_center(&hint.raw.bounds)?;
        }
    }
    Ok(())
}
