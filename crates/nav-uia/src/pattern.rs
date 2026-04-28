//! Pattern probes and interaction classification (Invoke, Toggle, Selection, Expand, Legacy, Value).

use nav_core::ElementKind;
use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
    UIA_LegacyIAccessiblePatternId, UIA_SelectionItemPatternId, UIA_TogglePatternId,
    UIA_ValuePatternId,
};

#[inline]
pub fn has_invoke_pattern_cached(el: &IUIAutomationElement) -> bool {
    unsafe { el.GetCachedPattern(UIA_InvokePatternId) }.is_ok()
}

#[inline]
pub fn has_invoke_pattern_current(el: &IUIAutomationElement) -> bool {
    unsafe { el.GetCurrentPattern(UIA_InvokePatternId) }.is_ok()
}

macro_rules! cached_pat {
    ($el:expr, $id:expr) => {
        unsafe { $el.GetCachedPattern($id) }.is_ok()
    };
}
macro_rules! current_pat {
    ($el:expr, $id:expr) => {
        unsafe { $el.GetCurrentPattern($id) }.is_ok()
    };
}

#[inline]
pub fn has_toggle_cached(el: &IUIAutomationElement) -> bool {
    cached_pat!(el, UIA_TogglePatternId)
}
#[inline]
pub fn has_toggle_current(el: &IUIAutomationElement) -> bool {
    current_pat!(el, UIA_TogglePatternId)
}

#[inline]
pub fn has_selection_item_cached(el: &IUIAutomationElement) -> bool {
    cached_pat!(el, UIA_SelectionItemPatternId)
}
#[inline]
pub fn has_selection_item_current(el: &IUIAutomationElement) -> bool {
    current_pat!(el, UIA_SelectionItemPatternId)
}

#[inline]
pub fn has_expand_collapse_cached(el: &IUIAutomationElement) -> bool {
    cached_pat!(el, UIA_ExpandCollapsePatternId)
}
#[inline]
pub fn has_expand_collapse_current(el: &IUIAutomationElement) -> bool {
    current_pat!(el, UIA_ExpandCollapsePatternId)
}

#[inline]
pub fn has_legacy_cached(el: &IUIAutomationElement) -> bool {
    cached_pat!(el, UIA_LegacyIAccessiblePatternId)
}
#[inline]
pub fn has_legacy_current(el: &IUIAutomationElement) -> bool {
    current_pat!(el, UIA_LegacyIAccessiblePatternId)
}

#[inline]
pub fn has_value_cached(el: &IUIAutomationElement) -> bool {
    cached_pat!(el, UIA_ValuePatternId)
}
#[inline]
pub fn has_value_current(el: &IUIAutomationElement) -> bool {
    current_pat!(el, UIA_ValuePatternId)
}

/// Best dispatch kind for this element (highest-priority pattern wins).
pub fn classify_interaction_kind(
    el: &IUIAutomationElement,
    from_cache: bool,
) -> Option<ElementKind> {
    if from_cache {
        if has_invoke_pattern_cached(el) {
            return Some(ElementKind::Invoke);
        }
        if has_toggle_cached(el) {
            return Some(ElementKind::Toggle);
        }
        if has_selection_item_cached(el) {
            return Some(ElementKind::Select);
        }
        if has_expand_collapse_cached(el) {
            return Some(ElementKind::ExpandCollapse);
        }
        if has_legacy_cached(el) {
            return Some(ElementKind::GenericClickable);
        }
        if has_value_cached(el) {
            return value_kind_from_control_type(el, true);
        }
    } else {
        if has_invoke_pattern_current(el) {
            return Some(ElementKind::Invoke);
        }
        if has_toggle_current(el) {
            return Some(ElementKind::Toggle);
        }
        if has_selection_item_current(el) {
            return Some(ElementKind::Select);
        }
        if has_expand_collapse_current(el) {
            return Some(ElementKind::ExpandCollapse);
        }
        if has_legacy_current(el) {
            return Some(ElementKind::GenericClickable);
        }
        if has_value_current(el) {
            return value_kind_from_control_type(el, false);
        }
    }
    keyboard_focusable_interactive(el, from_cache)
        .or_else(|| custom_control_candidate(el, from_cache))
}

fn custom_control_candidate(el: &IUIAutomationElement, from_cache: bool) -> Option<ElementKind> {
    use windows::Win32::UI::Accessibility::UIA_CustomControlTypeId;
    let ct = if from_cache {
        unsafe { el.CachedControlType() }.ok()?
    } else {
        unsafe { el.CurrentControlType() }.ok()?
    };
    if ct != UIA_CustomControlTypeId {
        return None;
    }
    let name_nonempty = if from_cache {
        unsafe { el.CachedName() }
            .ok()
            .map(|b| !b.to_string().is_empty())
            .unwrap_or(false)
    } else {
        unsafe { el.CurrentName() }
            .ok()
            .map(|b| !b.to_string().is_empty())
            .unwrap_or(false)
    };
    let kb = if from_cache {
        unsafe { el.CachedIsKeyboardFocusable() }
            .ok()
            .is_some_and(|b| b.as_bool())
    } else {
        unsafe { el.CurrentIsKeyboardFocusable() }
            .ok()
            .is_some_and(|b| b.as_bool())
    };
    if name_nonempty || kb {
        Some(ElementKind::GenericClickable)
    } else {
        None
    }
}

fn value_kind_from_control_type(
    el: &IUIAutomationElement,
    from_cache: bool,
) -> Option<ElementKind> {
    let ct = if from_cache {
        unsafe { el.CachedControlType() }.ok()?
    } else {
        unsafe { el.CurrentControlType() }.ok()?
    };
    use windows::Win32::UI::Accessibility::{
        UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    };
    if ct == UIA_EditControlTypeId
        || ct == UIA_DocumentControlTypeId
        || ct == UIA_ComboBoxControlTypeId
    {
        Some(ElementKind::Editable)
    } else {
        Some(ElementKind::GenericClickable)
    }
}

/// Focusable leaf controls without dedicated patterns (common in custom / Electron UIA bridges).
fn keyboard_focusable_interactive(
    el: &IUIAutomationElement,
    from_cache: bool,
) -> Option<ElementKind> {
    let focus = if from_cache {
        unsafe { el.CachedIsKeyboardFocusable() }.ok()?
    } else {
        unsafe { el.CurrentIsKeyboardFocusable() }.ok()?
    };
    if !focus.as_bool() {
        return None;
    }
    let ct = if from_cache {
        unsafe { el.CachedControlType() }.ok()?
    } else {
        unsafe { el.CurrentControlType() }.ok()?
    };
    use windows::Win32::UI::Accessibility::{
        UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId,
        UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_HyperlinkControlTypeId,
        UIA_ListItemControlTypeId, UIA_MenuItemControlTypeId, UIA_RadioButtonControlTypeId,
        UIA_SplitButtonControlTypeId, UIA_TabItemControlTypeId, UIA_TreeItemControlTypeId,
    };
    if ct == UIA_EditControlTypeId || ct == UIA_DocumentControlTypeId {
        return Some(ElementKind::Editable);
    }
    let interactive = ct == UIA_ButtonControlTypeId
        || ct == UIA_CheckBoxControlTypeId
        || ct == UIA_RadioButtonControlTypeId
        || ct == UIA_HyperlinkControlTypeId
        || ct == UIA_ListItemControlTypeId
        || ct == UIA_MenuItemControlTypeId
        || ct == UIA_TabItemControlTypeId
        || ct == UIA_TreeItemControlTypeId
        || ct == UIA_SplitButtonControlTypeId
        || ct == UIA_ComboBoxControlTypeId;
    if interactive {
        Some(ElementKind::GenericClickable)
    } else {
        None
    }
}
