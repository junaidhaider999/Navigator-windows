//! Pattern probes (baseline: invoke only).

use windows::Win32::UI::Accessibility::{IUIAutomationElement, UIA_InvokePatternId};

/// Whether the element exposes Invoke, using **cached** pattern data (`FindAllBuildCache` path).
#[inline]
pub fn has_invoke_pattern_cached(el: &IUIAutomationElement) -> bool {
    unsafe { el.GetCachedPattern(UIA_InvokePatternId) }.is_ok()
}

/// Whether the element exposes Invoke via **live** pattern query (`FindAll` fallback path).
#[inline]
pub fn has_invoke_pattern_current(el: &IUIAutomationElement) -> bool {
    unsafe { el.GetCurrentPattern(UIA_InvokePatternId) }.is_ok()
}
