//! `UiaRuntime`: COM apartment + `CUIAutomation8` (fallback `CUIAutomation`) singleton.

use nav_core::{Hint, RawHint};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest,
};

use crate::UiaError;
use crate::cache::{create_enumeration_cache_request, create_invoke_findall_cache_request};
use crate::enumerate::enumerate_baseline;
use crate::hwnd::UiaHwnd;
use crate::invoke::invoke_invoke_pattern;
use crate::options::{EnumOptions, FallbackPolicy};

/// UI Automation client (D1: shared enumeration cache request).
pub struct UiaRuntime {
    automation: IUIAutomation,
    enum_cache: IUIAutomationCacheRequest,
    /// `FindAllBuildCache` for invoke only (`AutomationElementMode_Full`); see `invoke.rs`.
    invoke_find_cache: IUIAutomationCacheRequest,
    /// Call [`CoUninitialize`](CoUninitialize) only if this instance successfully called `CoInitializeEx` first on this thread.
    co_uninit_on_drop: bool,
}

impl UiaRuntime {
    /// Initializes COM on this thread (STA) and creates the UI Automation singleton.
    pub fn new() -> Result<Self, UiaError> {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Err(UiaError::ComInit(hr.0));
        }
        if hr.is_err() {
            return Err(UiaError::ComInit(hr.0));
        }
        // `S_OK` (0) means we must balance with `CoUninitialize`. `S_FALSE` means COM was already initialized here.
        let co_uninit_on_drop = hr.0 == 0;

        let automation: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) } {
                Ok(a) => a,
                Err(e8) => {
                    match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
                        Ok(a) => a,
                        Err(e) => {
                            if co_uninit_on_drop {
                                unsafe { CoUninitialize() };
                            }
                            return Err(UiaError::AutomationCreate(format!(
                                "CUIAutomation8: {e8}; CUIAutomation: {e}"
                            )));
                        }
                    }
                }
            };

        let enum_cache = match create_enumeration_cache_request(&automation) {
            Ok(c) => c,
            Err(e) => {
                if co_uninit_on_drop {
                    unsafe { CoUninitialize() };
                }
                return Err(e);
            }
        };

        let invoke_find_cache = match create_invoke_findall_cache_request(&automation) {
            Ok(c) => c,
            Err(e) => {
                if co_uninit_on_drop {
                    unsafe { CoUninitialize() };
                }
                return Err(e);
            }
        };

        Ok(Self {
            automation,
            enum_cache,
            invoke_find_cache,
            co_uninit_on_drop,
        })
    }

    /// Enumerate invoke targets for the window captured at hotkey time (D1 cache).
    pub fn enumerate(&self, hwnd: UiaHwnd, opts: &EnumOptions) -> Result<Vec<RawHint>, UiaError> {
        if opts.fallback == FallbackPolicy::MsaaOnly {
            return Err(UiaError::UnsupportedConfiguration(
                "MsaaOnly is not implemented in the B3 baseline",
            ));
        }
        enumerate_baseline(&self.automation, hwnd, opts, &self.enum_cache)
    }

    /// Pattern dispatch: `Invoke` on the element located at the same `FindAll` index as enumeration.
    pub fn invoke(&self, hwnd: UiaHwnd, hint: &Hint) -> Result<(), UiaError> {
        invoke_invoke_pattern(&self.automation, hwnd, hint, &self.invoke_find_cache)
    }
}

impl Drop for UiaRuntime {
    fn drop(&mut self) {
        if self.co_uninit_on_drop {
            unsafe { CoUninitialize() };
        }
    }
}
