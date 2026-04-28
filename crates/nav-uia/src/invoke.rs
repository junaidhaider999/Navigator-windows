//! UIA dispatch: re-resolve the same `FindAllBuildCache` index as enumeration, then invoke pattern,
//! toggle, selection, expand/collapse, legacy default action, or physical click fallback.

use core::ffi::c_void;

use nav_core::{Backend, ElementKind, Hint, UiaEnumerateBasis};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Accessibility::{
    ExpandCollapseState_Collapsed, ExpandCollapseState_LeafNode,
    ExpandCollapseState_PartiallyExpanded, IUIAutomation, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationExpandCollapsePattern,
    IUIAutomationInvokePattern, IUIAutomationLegacyIAccessiblePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, TreeScope_Children,
    TreeScope_Descendants, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionItemPatternId, UIA_TabItemControlTypeId,
    UIA_TogglePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
use windows::core::Interface;

use crate::UiaError;
use crate::click::{invoke_click_hint, resolve_invoke_physical_point};
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

    dispatch_with_fallbacks(automation, &el, hint, hwnd)?;
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
    } else if hint.raw.backend == Backend::Uia
        && matches!(
            hint.raw.uia_enumerate_basis,
            UiaEnumerateBasis::RootChildrenOrder
        )
    {
        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|e| UiaError::Operation(e.to_string()))?;
        let kids = unsafe {
            root.FindAllBuildCache(TreeScope_Children, find_descendants_cond, find_cache)
        }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache Children (basis): {e}")))?;
        let len = unsafe { kids.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
        bounds_check(idx, len)?;
        unsafe { kids.GetElement(idx) }.map_err(|e| UiaError::Operation(e.to_string()))?
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

/// Resolve → physical click first for tab strips / Explorer shell where patterns only move focus →
/// kind-first dispatch → pattern ladder → `ElementFromPoint` → `SendInput` → `SetFocus` (editable only).
fn dispatch_with_fallbacks(
    automation: &IUIAutomation,
    el: &IUIAutomationElement,
    hint: &Hint,
    session_hwnd: UiaHwnd,
) -> Result<&'static str, UiaError> {
    // Tab items: Invoke / SelectionItem often only move keyboard focus; real click switches tabs.
    if let Ok(ct) = unsafe { el.CurrentControlType() } {
        if ct == UIA_TabItemControlTypeId && invoke_click_hint(&hint.raw).is_ok() {
            eprintln!(
                "[invoke] hint={} backend=UIA mode=SendInputClick TabItem-first",
                hint.label
            );
            return Ok("SendInputClick");
        }
    }
    if explorer_shell_selection_prefers_physical_click(session_hwnd, hint)
        && invoke_click_hint(&hint.raw).is_ok()
    {
        eprintln!(
            "[invoke] hint={} backend=UIA mode=SendInputClick ExplorerShell-Select-first",
            hint.label
        );
        return Ok("SendInputClick");
    }

    if let Ok(m) = dispatch_primary(el, hint) {
        return Ok(m);
    }
    if let Ok(m) = try_invoke_priority_ladder(el, hint) {
        return Ok(m);
    }
    if let Ok(ref el2) = element_at_invoke_point(automation, hint) {
        if let Ok(m) = dispatch_primary(el2, hint) {
            return Ok(m);
        }
        if let Ok(m) = try_invoke_priority_ladder(el2, hint) {
            return Ok(m);
        }
    }
    invoke_click_hint(&hint.raw)?;
    eprintln!("[invoke] hint={} fallback=SendInputClick", hint.label);
    Ok("SendInputClick")
}

fn explorer_shell_selection_prefers_physical_click(session_hwnd: UiaHwnd, hint: &Hint) -> bool {
    if hint.raw.kind != ElementKind::Select {
        return false;
    }
    let mut buf = [0u16; 96];
    let n = unsafe { GetClassNameW(session_hwnd, &mut buf) };
    if n == 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&buf[..n as usize]);
    let name = name.trim_end_matches('\0');
    name.eq_ignore_ascii_case("CabinetWClass") || name.eq_ignore_ascii_case("ExploreWClass")
}

fn element_at_invoke_point(
    automation: &IUIAutomation,
    hint: &Hint,
) -> Result<IUIAutomationElement, UiaError> {
    let (cx, cy) = resolve_invoke_physical_point(&hint.raw);
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

/// Ladder when [`dispatch_primary`] failed: Invoke → Legacy → semantic Toggle/Select/ExpandCollapse → never `SetFocus` here.
fn try_invoke_priority_ladder(
    el: &IUIAutomationElement,
    hint: &Hint,
) -> Result<&'static str, UiaError> {
    if try_invoke_only(el).is_ok() {
        eprintln!(
            "[invoke] hint={} backend=UIA mode=InvokePattern",
            hint.label
        );
        return Ok("InvokePattern");
    }
    if try_legacy_default_only(el).is_ok() {
        eprintln!(
            "[invoke] hint={} backend=UIA mode=LegacyIAccessible",
            hint.label
        );
        return Ok("LegacyIAccessible");
    }
    if hint.raw.kind == ElementKind::Toggle && try_toggle_only(el).is_ok() {
        eprintln!("[invoke] hint={} backend=UIA mode=Toggle", hint.label);
        return Ok("Toggle");
    }
    if hint.raw.kind == ElementKind::Select && try_select_only(el).is_ok() {
        eprintln!(
            "[invoke] hint={} backend=UIA mode=SelectionItem",
            hint.label
        );
        return Ok("SelectionItem");
    }
    if hint.raw.kind == ElementKind::ExpandCollapse && try_expand_collapse_semantic(el).is_ok() {
        eprintln!(
            "[invoke] hint={} backend=UIA mode=ExpandCollapse",
            hint.label
        );
        return Ok("ExpandCollapse");
    }
    Err(UiaError::Operation("invoke ladder exhausted".into()))
}

fn try_expand_collapse_semantic(el: &IUIAutomationElement) -> Result<(), UiaError> {
    let pat = pattern_cached_or_current(el, UIA_ExpandCollapsePatternId)?;
    let p: IUIAutomationExpandCollapsePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    let state = unsafe { p.CachedExpandCollapseState() }
        .or_else(|_| unsafe { p.CurrentExpandCollapseState() })
        .map_err(|e| UiaError::Operation(e.to_string()))?;
    if state == ExpandCollapseState_LeafNode {
        return Err(UiaError::Operation("expand leaf".into()));
    }
    if state == ExpandCollapseState_Collapsed || state == ExpandCollapseState_PartiallyExpanded {
        unsafe { p.Expand() }.map_err(|e| UiaError::Operation(e.to_string()))
    } else {
        unsafe { p.Collapse() }.map_err(|e| UiaError::Operation(e.to_string()))
    }
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
            eprintln!(
                "[invoke] hint={} backend=UIA mode=InvokePattern",
                hint.label
            );
            Ok("InvokePattern")
        }
        ElementKind::Toggle => {
            let pat = pattern_cached_or_current(el, UIA_TogglePatternId)?;
            let p: IUIAutomationTogglePattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Toggle() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            eprintln!("[invoke] hint={} backend=UIA mode=Toggle", hint.label);
            Ok("Toggle")
        }
        ElementKind::Select => {
            let pat = pattern_cached_or_current(el, UIA_SelectionItemPatternId)?;
            let p: IUIAutomationSelectionItemPattern =
                pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
            unsafe { p.Select() }.map_err(|e| UiaError::Operation(e.to_string()))?;
            eprintln!(
                "[invoke] hint={} backend=UIA mode=SelectionItem",
                hint.label
            );
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
                invoke_click_hint(&hint.raw)?;
                eprintln!("[invoke] hint={} fallback=SendInputClick", hint.label);
                Ok("SendInputClick")
            } else if state == ExpandCollapseState_Collapsed
                || state == ExpandCollapseState_PartiallyExpanded
            {
                unsafe { p.Expand() }.map_err(|e| UiaError::Operation(e.to_string()))?;
                eprintln!(
                    "[invoke] hint={} backend=UIA mode=ExpandCollapse",
                    hint.label
                );
                Ok("ExpandCollapse")
            } else {
                unsafe { p.Collapse() }.map_err(|e| UiaError::Operation(e.to_string()))?;
                eprintln!(
                    "[invoke] hint={} backend=UIA mode=ExpandCollapse",
                    hint.label
                );
                Ok("ExpandCollapse")
            }
        }
        ElementKind::GenericClickable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_InvokePatternId) {
                if let Ok(invoke) = pat.cast::<IUIAutomationInvokePattern>() {
                    if unsafe { invoke.Invoke() }.is_ok() {
                        eprintln!(
                            "[invoke] hint={} backend=UIA mode=InvokePattern",
                            hint.label
                        );
                        return Ok("InvokePattern");
                    }
                }
            }
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        eprintln!(
                            "[invoke] hint={} backend=UIA mode=LegacyIAccessible",
                            hint.label
                        );
                        return Ok("LegacyIAccessible");
                    }
                }
            }
            invoke_click_hint(&hint.raw)?;
            eprintln!("[invoke] hint={} fallback=SendInputClick", hint.label);
            Ok("SendInputClick")
        }
        ElementKind::Editable => {
            if let Ok(pat) = pattern_cached_or_current(el, UIA_LegacyIAccessiblePatternId) {
                if let Ok(leg) = pat.cast::<IUIAutomationLegacyIAccessiblePattern>() {
                    if unsafe { leg.DoDefaultAction() }.is_ok() {
                        eprintln!(
                            "[invoke] hint={} backend=UIA mode=LegacyIAccessible",
                            hint.label
                        );
                        return Ok("LegacyIAccessible");
                    }
                }
            }
            match invoke_click_hint(&hint.raw) {
                Ok(()) => {
                    eprintln!("[invoke] hint={} fallback=SendInputClick", hint.label);
                    Ok("SendInputClick")
                }
                Err(e_click) => {
                    unsafe { el.SetFocus() }.map_err(|e| {
                        UiaError::Operation(format!(
                            "SetFocus after SendInput failed ({e_click}); SetFocus: {e}"
                        ))
                    })?;
                    eprintln!("[invoke] hint={} backend=UIA mode=SetFocus", hint.label);
                    Ok("SetFocus")
                }
            }
        }
    }
}
