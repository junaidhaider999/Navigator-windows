//! [`IUIAutomationCacheRequest`] builders: fast enumeration vs invoke-time `FindAllBuildCache`.

use windows::Win32::UI::Accessibility::{
    AutomationElementMode_Full, AutomationElementMode_None, IUIAutomation,
    IUIAutomationCacheRequest, TreeScope_Element, UIA_BoundingRectanglePropertyId,
    UIA_InvokePatternId, UIA_IsEnabledPropertyId, UIA_IsOffscreenPropertyId, UIA_NamePropertyId,
};

use crate::UiaError;

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
