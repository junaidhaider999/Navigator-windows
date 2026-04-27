//! `UiaRuntime`: COM apartment + `CUIAutomation8` (fallback `CUIAutomation`) singleton.

use std::sync::Mutex;

use nav_core::{Hint, NavEnumerateResult};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition,
};

use crate::UiaError;
use crate::cache::{
    create_enumeration_cache_request, create_invoke_findall_cache_request,
    create_invoke_targets_find_condition,
};
use crate::enumerate::enumerate_baseline;
use crate::hwnd::UiaHwnd;
use crate::invoke::invoke_invoke_pattern;
use crate::options::{EnumOptions, FallbackPolicy};

struct CachedFindDescendantsCondition {
    include_disabled: bool,
    include_offscreen: bool,
    condition: IUIAutomationCondition,
}

/// UI Automation client (D1: shared enumeration cache request).
pub struct UiaRuntime {
    automation: IUIAutomation,
    enum_cache: IUIAutomationCacheRequest,
    /// `FindAllBuildCache` for invoke only (`AutomationElementMode_Full`); see `invoke.rs`.
    invoke_find_cache: IUIAutomationCacheRequest,
    /// Last compound `FindAll` condition for descendants (matches `EnumOptions` filters).
    find_descendants_cond_cache: Mutex<Option<CachedFindDescendantsCondition>>,
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
            find_descendants_cond_cache: Mutex::new(None),
            co_uninit_on_drop,
        })
    }

    fn find_descendants_condition(
        &self,
        opts: &EnumOptions,
    ) -> Result<IUIAutomationCondition, UiaError> {
        let mut guard = self.find_descendants_cond_cache.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            if c.include_disabled == opts.include_disabled
                && c.include_offscreen == opts.include_offscreen
            {
                return Ok(c.condition.clone());
            }
        }
        let condition = match create_invoke_targets_find_condition(&self.automation, opts) {
            Ok(c) => c,
            Err(_) => unsafe { self.automation.CreateTrueCondition() }.map_err(|e| {
                UiaError::Operation(format!("CreateTrueCondition (find fallback): {e}"))
            })?,
        };
        *guard = Some(CachedFindDescendantsCondition {
            include_disabled: opts.include_disabled,
            include_offscreen: opts.include_offscreen,
            condition: condition.clone(),
        });
        Ok(condition)
    }

    /// Enumerate actionable UI for the window captured at hotkey time (D1 cache).
    ///
    /// When [`EnumOptions::debug_overlay`](crate::options::EnumOptions::debug_overlay) is set,
    /// `debug_rejects` lists nodes that matched the provider filter but were dropped in Rust
    /// (for visualization).
    pub fn enumerate(
        &self,
        hwnd: UiaHwnd,
        opts: &EnumOptions,
    ) -> Result<NavEnumerateResult, UiaError> {
        if opts.fallback == FallbackPolicy::MsaaOnly {
            return Err(UiaError::UnsupportedConfiguration(
                "MsaaOnly is not implemented in the B3 baseline",
            ));
        }
        let find_cond = self.find_descendants_condition(opts)?;
        enumerate_baseline(&self.automation, hwnd, opts, &self.enum_cache, &find_cond)
    }

    /// Pattern dispatch: `Invoke` on the element located at the same `FindAll` index as enumeration.
    ///
    /// `opts` must match the [`EnumOptions`] used for the preceding [`UiaRuntime::enumerate`] call
    /// so descendant filtering stays consistent with `element_id`.
    pub fn invoke(&self, hwnd: UiaHwnd, hint: &Hint, opts: &EnumOptions) -> Result<(), UiaError> {
        let find_cond = self.find_descendants_condition(opts)?;
        invoke_invoke_pattern(
            &self.automation,
            hwnd,
            hint,
            &self.invoke_find_cache,
            &find_cond,
        )
    }
}

impl Drop for UiaRuntime {
    fn drop(&mut self) {
        if self.co_uninit_on_drop {
            unsafe { CoUninitialize() };
        }
    }
}
