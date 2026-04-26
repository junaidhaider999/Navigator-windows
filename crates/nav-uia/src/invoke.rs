//! Invoke pattern dispatch (baseline: re-walk the same `FindAll` slice as enumeration).

use nav_core::Hint;
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationInvokePattern, TreeScope_Descendants, UIA_InvokePatternId,
};
use windows::core::Interface;

use crate::UiaError;
use crate::hwnd::UiaHwnd;

/// Invokes the element identified by [`Hint::raw`](nav_core::RawHint) `element_id` (enumeration index).
pub fn invoke_invoke_pattern(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    hint: &Hint,
) -> Result<(), UiaError> {
    if hwnd.is_invalid() {
        return Err(UiaError::Operation("invalid HWND for invoke".into()));
    }

    let idx = hint.raw.element_id;
    if idx > i32::MAX as u64 {
        return Err(UiaError::Operation("element_id out of range".into()));
    }
    let idx = idx as i32;

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;
    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;
    let all = unsafe { root.FindAll(TreeScope_Descendants, &true_cond) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;
    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;
    if idx < 0 || idx >= len {
        return Err(UiaError::Operation(format!(
            "invoke index {idx} out of bounds (len={len})"
        )));
    }

    let el = unsafe { all.GetElement(idx) }.map_err(|e| UiaError::Operation(e.to_string()))?;
    let pat = unsafe { el.GetCurrentPattern(UIA_InvokePatternId) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;
    let invoke: IUIAutomationInvokePattern =
        pat.cast().map_err(|e| UiaError::Operation(e.to_string()))?;
    unsafe { invoke.Invoke() }.map_err(|e| UiaError::Operation(e.to_string()))?;
    Ok(())
}
