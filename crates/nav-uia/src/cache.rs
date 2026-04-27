//! One-shot [`IUIAutomationCacheRequest`] for enumeration (D1).

use windows::Win32::UI::Accessibility::{
    AutomationElementMode_None, IUIAutomation, IUIAutomationCacheRequest, TreeScope_Element,
    UIA_BoundingRectanglePropertyId, UIA_InvokePatternId, UIA_IsEnabledPropertyId,
    UIA_IsOffscreenPropertyId, UIA_NamePropertyId,
};

use crate::UiaError;

/// Cache request for [`IUIAutomationElement::FindAllBuildCache`].
///
/// Microsoft requires [`TreeScope_Element`](windows::Win32::UI::Accessibility::TreeScope_Element)
/// on the request when used with `FindAllBuildCache` (not `TreeScope_Descendants`).
///
/// `AutomationElementMode_None` halves the per-element bridge cost: UIA returns lightweight
/// proxy elements that *only* expose the cached properties/patterns we asked for. We never need
/// the full element interface during enumeration — the only later live op is `Invoke`, which
/// re-resolves elements on its own path. See `Agent/workflow/05-performance-strategy.md` (Win 1).
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
