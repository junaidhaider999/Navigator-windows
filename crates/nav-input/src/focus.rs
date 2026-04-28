//! Detect whether keyboard focus is in a control where typing plain `/` should reach the app.

use std::sync::Once;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, UIA_ComboBoxControlTypeId, UIA_EditControlTypeId,
};

static COM_INIT: Once = Once::new();

pub(crate) fn ensure_com_apartment() {
    COM_INIT.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    });
}

/// Returns `true` when focus is in a single-line edit or combo (typed `/` should reach the field).
///
/// We intentionally **do not** treat `Document` (e.g. browser page UIA tree) as typing, so plain `/`
/// still activates over web content like Vimium; use `Alt+;` when `/` must go to the page.
pub(crate) fn focused_control_suppresses_plain_slash_hotkey() -> bool {
    ensure_com_apartment();
    let automation = unsafe {
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    }
    .or_else(|_| unsafe {
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    });
    let Ok(automation) = automation else {
        return false;
    };
    let Ok(el) = (unsafe { automation.GetFocusedElement() }) else {
        return false;
    };
    let Ok(ct) = (unsafe { el.CurrentControlType() }) else {
        return false;
    };
    ct == UIA_EditControlTypeId || ct == UIA_ComboBoxControlTypeId
}
