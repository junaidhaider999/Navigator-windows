//! [`IUIAutomationCacheRequest`] builders: fast enumeration vs invoke-time `FindAllBuildCache`.
//! Also builds **FindAll** conditions so the provider pre-filters toward actionable UI (HAP parity).

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_I4};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_Full, AutomationElementMode_None, IUIAutomation,
    IUIAutomationCacheRequest, IUIAutomationCondition, TreeScope_Element, UIA_PROPERTY_ID,
    UIA_BoundingRectanglePropertyId, UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId,
    UIA_ComboBoxControlTypeId, UIA_ControlTypePropertyId, UIA_EditControlTypeId,
    UIA_ExpandCollapsePatternId, UIA_HyperlinkControlTypeId, UIA_InvokePatternId,
    UIA_IsEnabledPropertyId, UIA_IsExpandCollapsePatternAvailablePropertyId,
    UIA_IsInvokePatternAvailablePropertyId, UIA_IsKeyboardFocusablePropertyId,
    UIA_IsLegacyIAccessiblePatternAvailablePropertyId, UIA_IsOffscreenPropertyId,
    UIA_IsSelectionItemPatternAvailablePropertyId, UIA_IsTogglePatternAvailablePropertyId,
    UIA_IsValuePatternAvailablePropertyId, UIA_LegacyIAccessiblePatternId, UIA_ListItemControlTypeId,
    UIA_MenuItemControlTypeId, UIA_NamePropertyId, UIA_RadioButtonControlTypeId,
    UIA_SelectionItemPatternId, UIA_SplitButtonControlTypeId, UIA_TabItemControlTypeId,
    UIA_TogglePatternId, UIA_TreeItemControlTypeId, UIA_ValuePatternId,
};

use crate::UiaError;
use crate::options::EnumOptions;

fn variant_bool(vt_bool: windows::Win32::Foundation::VARIANT_BOOL) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { boolVal: vt_bool },
            }),
        },
    }
}

fn variant_i4(n: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: n },
            }),
        },
    }
}

fn or_property_flags(
    automation: &IUIAutomation,
    property_ids: &[UIA_PROPERTY_ID],
    v_true: &VARIANT,
) -> Result<IUIAutomationCondition, UiaError> {
    let Some(&first) = property_ids.first() else {
        return Err(UiaError::Operation("empty OR property list".into()));
    };
    let mut acc = unsafe { automation.CreatePropertyCondition(first, v_true) }
        .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition: {e}")))?;
    for &pid in &property_ids[1..] {
        let c = unsafe { automation.CreatePropertyCondition(pid, v_true) }
            .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition: {e}")))?;
        acc = unsafe { automation.CreateOrCondition(&acc, &c) }
            .map_err(|e| UiaError::Operation(format!("CreateOrCondition: {e}")))?;
    }
    Ok(acc)
}

fn or_control_types(
    automation: &IUIAutomation,
    types: &[i32],
    v_i4: impl Fn(i32) -> VARIANT,
) -> Result<IUIAutomationCondition, UiaError> {
    let Some(&t0) = types.first() else {
        return Err(UiaError::Operation("empty control type OR".into()));
    };
    let mut acc = unsafe {
        automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &v_i4(t0))
    }
    .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition ControlType: {e}")))?;
    for &t in &types[1..] {
        let c = unsafe {
            automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &v_i4(t))
        }
        .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition ControlType: {e}")))?;
        acc = unsafe { automation.CreateOrCondition(&acc, &c) }
            .map_err(|e| UiaError::Operation(format!("CreateOrCondition ControlType: {e}")))?;
    }
    Ok(acc)
}

/// `FindAll` / `FindAllBuildCache` for **descendants**: broad interaction OR (patterns + focusable
/// common control types), then optional enabled / on-screen AND clauses from `opts`.
pub fn create_invoke_targets_find_condition(
    automation: &IUIAutomation,
    opts: &EnumOptions,
) -> Result<IUIAutomationCondition, UiaError> {
    unsafe {
        let v_true = variant_bool(VARIANT_TRUE);
        let v_false = variant_bool(VARIANT_FALSE);

        let pattern_props = [
            UIA_IsInvokePatternAvailablePropertyId,
            UIA_IsTogglePatternAvailablePropertyId,
            UIA_IsSelectionItemPatternAvailablePropertyId,
            UIA_IsExpandCollapsePatternAvailablePropertyId,
            UIA_IsLegacyIAccessiblePatternAvailablePropertyId,
            UIA_IsValuePatternAvailablePropertyId,
        ];
        let pattern_or = or_property_flags(automation, &pattern_props, &v_true)?;

        let focus_types = [
            UIA_ButtonControlTypeId.0,
            UIA_CheckBoxControlTypeId.0,
            UIA_RadioButtonControlTypeId.0,
            UIA_HyperlinkControlTypeId.0,
            UIA_ListItemControlTypeId.0,
            UIA_MenuItemControlTypeId.0,
            UIA_TabItemControlTypeId.0,
            UIA_TreeItemControlTypeId.0,
            UIA_SplitButtonControlTypeId.0,
            UIA_ComboBoxControlTypeId.0,
            UIA_EditControlTypeId.0,
        ];
        let ct_or = or_control_types(automation, &focus_types, variant_i4)?;
        let kb = automation
            .CreatePropertyCondition(UIA_IsKeyboardFocusablePropertyId, &v_true)
            .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition KeyboardFocusable: {e}")))?;
        let focus_clickable = automation
            .CreateAndCondition(&kb, &ct_or)
            .map_err(|e| UiaError::Operation(format!("CreateAndCondition focus branch: {e}")))?;

        let mut acc = automation
            .CreateOrCondition(&pattern_or, &focus_clickable)
            .map_err(|e| UiaError::Operation(format!("CreateOrCondition pattern|focus: {e}")))?;

        if !opts.include_disabled {
            let c = automation
                .CreatePropertyCondition(UIA_IsEnabledPropertyId, &v_true)
                .map_err(|e| {
                    UiaError::Operation(format!("CreatePropertyCondition IsEnabled: {e}"))
                })?;
            acc = automation
                .CreateAndCondition(&acc, &c)
                .map_err(|e| UiaError::Operation(format!("CreateAndCondition (enabled): {e}")))?;
        }

        if !opts.include_offscreen {
            let c = automation
                .CreatePropertyCondition(UIA_IsOffscreenPropertyId, &v_false)
                .map_err(|e| {
                    UiaError::Operation(format!("CreatePropertyCondition IsOffscreen: {e}"))
                })?;
            acc = automation
                .CreateAndCondition(&acc, &c)
                .map_err(|e| UiaError::Operation(format!("CreateAndCondition (offscreen): {e}")))?;
        }

        Ok(acc)
    }
}

fn add_cache_patterns_and_properties(
    req: &IUIAutomationCacheRequest,
    mode_label: &str,
) -> Result<(), UiaError> {
    unsafe {
        req.AddProperty(UIA_BoundingRectanglePropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty BoundingRectangle: {e}")))?;
        req.AddProperty(UIA_IsEnabledPropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty IsEnabled: {e}")))?;
        req.AddProperty(UIA_IsOffscreenPropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty IsOffscreen: {e}")))?;
        req.AddProperty(UIA_NamePropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty Name: {e}")))?;
        req.AddProperty(UIA_ControlTypePropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty ControlType: {e}")))?;
        req.AddProperty(UIA_IsKeyboardFocusablePropertyId)
            .map_err(|e| UiaError::Operation(format!("{mode_label} AddProperty IsKeyboardFocusable: {e}")))?;
        for (label, pid) in [
            ("Invoke", UIA_InvokePatternId),
            ("Toggle", UIA_TogglePatternId),
            ("SelectionItem", UIA_SelectionItemPatternId),
            ("ExpandCollapse", UIA_ExpandCollapsePatternId),
            ("LegacyIAccessible", UIA_LegacyIAccessiblePatternId),
            ("Value", UIA_ValuePatternId),
        ] {
            req.AddPattern(pid).map_err(|e| {
                UiaError::Operation(format!("{mode_label} AddPattern {label}: {e}"))
            })?;
        }
    }
    Ok(())
}

/// Cache request for [`IUIAutomationElement::FindAllBuildCache`] during **enumeration**.
pub fn create_enumeration_cache_request(
    automation: &IUIAutomation,
) -> Result<IUIAutomationCacheRequest, UiaError> {
    unsafe {
        let req = automation
            .CreateCacheRequest()
            .map_err(|e| UiaError::Operation(format!("CreateCacheRequest: {e}")))?;
        req.SetTreeScope(TreeScope_Element)
            .map_err(|e| UiaError::Operation(format!("cache SetTreeScope: {e}")))?;
        req.SetAutomationElementMode(AutomationElementMode_None)
            .map_err(|e| UiaError::Operation(format!("cache SetAutomationElementMode: {e}")))?;
        add_cache_patterns_and_properties(&req, "cache")?;
        Ok(req)
    }
}

/// Full-mode cache for the same `FindAllBuildCache` index as enumeration (dispatch / patterns).
pub fn create_invoke_findall_cache_request(
    automation: &IUIAutomation,
) -> Result<IUIAutomationCacheRequest, UiaError> {
    unsafe {
        let req = automation
            .CreateCacheRequest()
            .map_err(|e| UiaError::Operation(format!("CreateCacheRequest (invoke find): {e}")))?;
        req.SetTreeScope(TreeScope_Element)
            .map_err(|e| UiaError::Operation(format!("invoke find cache SetTreeScope: {e}")))?;
        req.SetAutomationElementMode(AutomationElementMode_Full)
            .map_err(|e| {
                UiaError::Operation(format!("invoke find cache SetAutomationElementMode: {e}"))
            })?;
        add_cache_patterns_and_properties(&req, "invoke find cache")?;
        Ok(req)
    }
}
