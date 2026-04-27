//! UIA dispatch: re-resolve the same `FindAllBuildCache` index as enumeration, then invoke pattern,
//! toggle, selection, expand/collapse, legacy default action, or physical click fallback.

use core::ffi::c_void;

use nav_core::{ElementKind, Hint};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Accessibility::{
    ExpandCollapseState_Collapsed, ExpandCollapseState_LeafNode,
    ExpandCollapseState_PartiallyExpanded, IUIAutomation, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationExpandCollapsePattern,
    IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, TreeScope_Children,
    TreeScope_Descendants, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionItemPatternId, UIA_TogglePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
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

    let fg = unsafe { GetForegroundWindow() };
    if fg != hwnd {
        eprintln!(
            "[invoke] warn: foreground_hwnd=0x{:x} session_hwnd=0x{:x}",
            fg.0 as usize, hwnd.0 as usize
        );
    }

    let idx = hint.raw.element_id;
    if idx > i32::MAX as u64 {
        return Err(UiaError::Operation("element_id out of range".into()));
    }
    let idx = idx as i32;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let el = resolve_enumerated_element(
        automation,
        hwnd,
        idx,
        &true_cond,
        find_cache,
        find_descendants_cond,
        hint,
    )?;

    let method = dispatch_with_fallbacks(automation, &el, hint)?;
    eprintln!(
        "[invoke] label={} backend=UIA method={} success=true",
        hint.label, method
    );
    Ok(())
}

fn resolve_enumerated_element(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    idx: i32,
    true_cond: &IUIAutomationCondition,
    find_cache: &IUIAutomationCacheRequest,
    find_descendants_cond: &IUIAutomationCondition,
    hint: &Hint,
) -> Result<IUIAutomationElement, UiaError> {
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
        let kids = unsafe { root.FindAllBuildCache(TreeScope_Children, true_cond, find_cache) }
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
    Ok(el)
}

/// Primary dispatch from enumeration classification, then universal pattern chain, `ElementFromPoint`, `SendInput`.
fn dispatch_with_fallbacks(
    automation: &IUIAutomation,
    el: &IUIAutomationElement,
    hint: &Hint,
) -> Result<&'static str, UiaError> {
    if let Ok(m) = dispatch_primary(el, hint) {
        return Ok(m);
    }
    if try_universal_action_chain(el).is_ok() {
        return Ok("pattern_fallback");
    }
    if let Ok(ref el2) = element_at_hint_center(automation, hint) {
        if let Ok(m) = dispatch_primary(el2, hint) {
            return Ok(m);
        }
        if try_universal_action_chain(el2).is_ok() {
            return Ok("element_from_point+fallback");
        }
    }
    left_click_rect_center(&hint.raw.bounds)?;
    Ok("SendInput")
}

fn element_at_hint_center(
    automation: &IUIAutomation,
    hint: &Hint,
) -> Result<IUIAutomationElement, UiaError> {
    let r = hint.raw.bounds;
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    unsafe { automation.ElementFromPoint(POINT { x: cx, y: cy }) }
        .map_err(|e| UiaError::Operation(format!("ElementFromPoint({cx},{cy}): {e}")))
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
            UiaError::Operation(format!("GetCachedPattern: {e1}; GetCurrentPattern: {e2}"))
        }),
    }
}

fn try_universal_action_chain(el: &IUIAutomationElement) -> Result<(), UiaError> {
    if try_invoke_only(el).is_ok() {
        return Ok(());
    }
    if try_select_only(el).is_ok() {
        return Ok(());
    }
    if try_toggle_only(el).is_ok() {
        return Ok(());
    }
    if try_legacy_default_only(el).is_ok() {
        return Ok(());
    }
    unsafe { el.SetFocus() }.map_err(|e| UiaError::Operation(format!("SetFocus: {e}")))
}

fn try_invoke_only(el: &IUIAutomationElement) -> Result<(), UiaError> {
    let pat = pattern_cached_or_current(el, UIA_InvokePatternId)?;
    let invoke: IUIAutomationInvokePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { invoke.Invoke() }.map_err(|e| UiaError::Operation(e.to_string()))
}

fn try_select_only(el: &IUIAutomationElement) -> Result<(), UiaError> {
    let pat = pattern_cached_or_current(el, UIA_SelectionItemPatternId)?;
    let p: IUIAutomationSelectionItemPattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { p.Select() }.map_err(|e| UiaError::Operation(e.to_string()))
}

fn try_toggle_only(el: &IUIAutomationElement) -> Result<(), UiaError> {
    let pat = pattern_cached_or_current(el, UIA_TogglePatternId)?;
    let p: IUIAutomationTogglePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { p.Toggle() }.map_err(|e| UiaError::Operation(e.to_string()))
}

fn try_legacy_default_only(el: &IUIAutomationElement) -> Result<(), UiaError> {
    let pat = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId)?;
    let leg: IUIAutomationLegacyIAccessiblePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { leg.DoDefaultAction() }.map_err(|e| UiaError::Operation(e.to_string()))
}

/// Kind-first dispatch (`?` only when this kind requires that pattern to exist).
fn dispatch_primary(el: &IUIAutomationElement, hint: &Hint) -> Result<&'static str, UiaError> {
    match hint.raw.kind {
        ElementKind::Invoke => {
            let pat = pattern_cached_or_current(el, UIA_InvokePatternId)?;
            let invoke: IUIAutomationInvokePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { invoke.Invoke() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            Ok("Invoke")
        }
        ElementKind::Toggle => {
            let pat = pattern_cached_or_current(el, UIA_TogglePatternId)?;
            let p: IUIAutomationTogglePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Toggle() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            Ok("Toggle")
        }
        ElementKind::Select => {
            let pat = pattern_cached_or_current(el, UIA_SelectionItemPatternId)?;
            let p: IUIAutomationSelectionItemPattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Select() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            Ok("SelectionItem")
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
                Ok("SendInput")
            } else if state == ExpandCollapseState_Collapsed
                || state == ExpandCollapseState_PartiallyExpanded
            {
                unsafe { p.Expand() }.map_err(|e| UiaError::Operation(e.to_string()))?;
                Ok("Expand")
            } else {
                unsafe { p.Collapse() }.map_err(|e| UiaError::Operation(e.to_string()))?;
                Ok("Collapse")
            }
        }
        ElementKind::GenericClickable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        return Ok("LegacyIAccessible");
                    }
                }
            }
            left_click_rect_center(&hint.raw.bounds)?;
            Ok("SendInput")
        }
        ElementKind::Editable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        return Ok("LegacyIAccessible");
                    }
                }
            }
            left_click_rect_center(&hint.raw.bounds)?;
            Ok("SendInput")
        }
    }
}
