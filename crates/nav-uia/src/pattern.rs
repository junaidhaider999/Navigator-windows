//! Pattern probes (baseline: invoke only).

use windows::Win32::UI::Accessibility::{IUIAutomationElement, UIA_InvokePatternId};

/// Returns whether the element currently exposes the Invoke control pattern.
pub fn has_invoke_pattern(el: &IUIAutomationElement) -> bool {
    unsafe { el.GetCurrentPattern(UIA_InvokePatternId) }.is_ok()
}
