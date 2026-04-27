//! [`IUIAutomationCacheRequest`] builders: fast enumeration vs invoke-time `FindAllBuildCache`.
//! Also builds **FindAll** conditions that match [`crate::options::EnumOptions`] so the provider
//! walks fewer nodes than `CreateTrueCondition()`.

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL};
use windows::Win32::UI::Accessibility::{
    AutomationElementMode_Full, AutomationElementMode_None, IUIAutomation,
    IUIAutomationCacheRequest, IUIAutomationCondition, TreeScope_Element,
    UIA_BoundingRectanglePropertyId, UIA_IsEnabledPropertyId,
    UIA_IsInvokePatternAvailablePropertyId, UIA_IsOffscreenPropertyId, UIA_InvokePatternId,
    UIA_NamePropertyId,
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

/// `FindAll` / `FindAllBuildCache` condition for **descendants**: invoke pattern available, plus
/// enabled / on-screen filters implied by `opts`. Falls back to [`CreateTrueCondition`](IUIAutomation::CreateTrueCondition)
/// if building compound conditions fails (rare).
pub fn create_invoke_targets_find_condition(
    automation: &IUIAutomation,
    opts: &EnumOptions,
) -> Result<IUIAutomationCondition, UiaError> {
    unsafe {
        let v_true = variant_bool(VARIANT_TRUE);
        let v_false = variant_bool(VARIANT_FALSE);

        let mut acc: IUIAutomationCondition = automation
            .CreatePropertyCondition(UIA_IsInvokePatternAvailablePropertyId, &v_true)
            .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition IsInvokeAvailable: {e}")))?;

        if !opts.include_disabled {
            let c = automation
                .CreatePropertyCondition(UIA_IsEnabledPropertyId, &v_true)
                .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition IsEnabled: {e}")))?;
            acc = automation
                .CreateAndCondition(&acc, &c)
                .map_err(|e| UiaError::Operation(format!("CreateAndCondition (enabled): {e}")))?;
        }

        if !opts.include_offscreen {
            let c = automation
                .CreatePropertyCondition(UIA_IsOffscreenPropertyId, &v_false)
                .map_err(|e| UiaError::Operation(format!("CreatePropertyCondition IsOffscreen: {e}")))?;
            acc = automation
                .CreateAndCondition(&acc, &c)
                .map_err(|e| UiaError::Operation(format!("CreateAndCondition (offscreen): {e}")))?;
        }

        Ok(acc)
    }
}

/// Cache request for [`IUIAutomationElement::FindAllBuildCache`] during **enumeration**.
///
/// Microsoft requires [`TreeScope_Element`](windows::Win32::UI::Accessibility::TreeScope_Element)
/// on the request when used with `FindAllBuildCache` (not `TreeScope_Descendants`).
///
/// `AutomationElementMode_None` reduces per-element cost. Elements from that snapshot are not
/// usable for `Invoke` — invoke re-runs `FindAllBuildCache` with [`create_invoke_findall_cache_request`].
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
        req.AddProperty(UIA_BoundingRectanglePropertyId)
            .map_err(|e| {
                UiaError::Operation(format!("cache AddProperty BoundingRectangle: {e}"))
            })?;
        req.AddProperty(UIA_IsEnabledPropertyId)
            .map_err(|e| UiaError::Operation(format!("cache AddProperty IsEnabled: {e}")))?;
        req.AddProperty(UIA_IsOffscreenPropertyId)
            .map_err(|e| UiaError::Operation(format!("cache AddProperty IsOffscreen: {e}")))?;
        req.AddProperty(UIA_NamePropertyId)
            .map_err(|e| UiaError::Operation(format!("cache AddProperty Name: {e}")))?;
        req.AddPattern(UIA_InvokePatternId)
            .map_err(|e| UiaError::Operation(format!("cache AddPattern Invoke: {e}")))?;
        Ok(req)
    }
}

/// Same properties/patterns as enumeration, but **`AutomationElementMode_Full`** so
/// `IUIAutomationElementArray::GetElement` returns a real element (`GetCachedPattern` /
/// `GetCurrentPattern` / `Invoke` work).
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
        req.AddProperty(UIA_BoundingRectanglePropertyId)
            .map_err(|e| {
                UiaError::Operation(format!(
                    "invoke find cache AddProperty BoundingRectangle: {e}"
                ))
            })?;
        req.AddProperty(UIA_IsEnabledPropertyId).map_err(|e| {
            UiaError::Operation(format!("invoke find cache AddProperty IsEnabled: {e}"))
        })?;
        req.AddProperty(UIA_IsOffscreenPropertyId).map_err(|e| {
            UiaError::Operation(format!("invoke find cache AddProperty IsOffscreen: {e}"))
        })?;
        req.AddProperty(UIA_NamePropertyId)
            .map_err(|e| UiaError::Operation(format!("invoke find cache AddProperty Name: {e}")))?;
        req.AddPattern(UIA_InvokePatternId).map_err(|e| {
            UiaError::Operation(format!("invoke find cache AddPattern Invoke: {e}"))
        })?;
        Ok(req)
    }
}
