//! `UiaRuntime`: COM apartment + `CUIAutomation8` (fallback `CUIAutomation`) singleton.
//!
//! ## M9 fallback ladder (`FallbackPolicy::Auto`)
//!
//! 1. **UIA** — `FindAllBuildCache` (or `FindAll` on pattern-cache build failure) with
//!    [`EnumOptions::budget_uia_ms`](crate::options::EnumOptions::budget_uia_ms).
//! 2. If no hints: **MSAA** — `AccessibleObjectFromWindow` + DFS; budget
//!    [`EnumOptions::budget_msaa_ms`].
//! 3. If still no hints: **raw HWND** — `EnumChildWindows` + `GetWindowRect`; budget
//!    [`EnumOptions::budget_hwnd_ms`].
//!
//! `MsaaOnly` and `UiaOnly` run a single stage (with the matching budget where applicable).

use std::sync::Mutex;

use nav_core::{Backend, Hint, NavEnumerateResult};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::UiaError;
use crate::cache::{
    create_enumeration_cache_request, create_invoke_findall_cache_request,
    create_invoke_targets_find_condition,
};
use crate::click::invoke_click_hint;
use crate::enumerate::enumerate_baseline;
use crate::fallback_hwnd::enumerate_raw_hwnd;
use crate::fallback_msaa::{enumerate_msaa, invoke_msaa_at};
use crate::hwnd::UiaHwnd;
use crate::invoke::invoke_invoke_pattern;
use crate::options::{EnumOptions, EnumerationProfile, FallbackPolicy};

fn qpc_delta_ms(freq: i64, t0: i64, t1: i64) -> f64 {
    if freq <= 0 {
        return 0.0;
    }
    (t1.saturating_sub(t0) as f64) * 1000.0 / freq as f64
}

fn budget_warn(stage: &str, took_ms: f64, budget_ms: u64) {
    if budget_ms == 0 || took_ms <= budget_ms as f64 {
        return;
    }
    eprintln!(
        "[uia] budget: stage={} took_ms={:.2} (soft budget {} ms)",
        stage, took_ms, budget_ms
    );
}

struct CachedFindDescendantsCondition {
    include_disabled: bool,
    include_offscreen: bool,
    profile: EnumerationProfile,
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
                && c.profile == opts.profile
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
            profile: opts.profile,
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
        let mut freq = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut freq);
        }

        match opts.fallback {
            FallbackPolicy::MsaaOnly => {
                let mut t0 = 0i64;
                let mut t1 = 0i64;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t0);
                }
                let hints = enumerate_msaa(hwnd, opts)?;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t1);
                }
                budget_warn("msaa", qpc_delta_ms(freq, t0, t1), opts.budget_msaa_ms);
                Ok(NavEnumerateResult {
                    hints,
                    debug_rejects: Vec::new(),
                    timings_ms: None,
                })
            }
            FallbackPolicy::UiaOnly => {
                let find_cond = self.find_descendants_condition(opts)?;
                let mut t0 = 0i64;
                let mut t1 = 0i64;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t0);
                }
                let res =
                    enumerate_baseline(&self.automation, hwnd, opts, &self.enum_cache, &find_cond);
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t1);
                }
                budget_warn("uia", qpc_delta_ms(freq, t0, t1), opts.budget_uia_ms);
                res
            }
            FallbackPolicy::Auto => {
                let find_cond = self.find_descendants_condition(opts)?;

                let mut t_uia_0 = 0i64;
                let mut t_uia_1 = 0i64;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_uia_0);
                }
                let r =
                    enumerate_baseline(&self.automation, hwnd, opts, &self.enum_cache, &find_cond)?;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_uia_1);
                }
                budget_warn(
                    "uia",
                    qpc_delta_ms(freq, t_uia_0, t_uia_1),
                    opts.budget_uia_ms,
                );

                if !r.hints.is_empty() {
                    return Ok(r);
                }

                let mut t_msaa_0 = 0i64;
                let mut t_msaa_1 = 0i64;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_msaa_0);
                }
                let msaa = enumerate_msaa(hwnd, opts)?;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_msaa_1);
                }
                budget_warn(
                    "msaa",
                    qpc_delta_ms(freq, t_msaa_0, t_msaa_1),
                    opts.budget_msaa_ms,
                );
                if !msaa.is_empty() {
                    return Ok(NavEnumerateResult {
                        hints: msaa,
                        debug_rejects: r.debug_rejects,
                        timings_ms: None,
                    });
                }

                let mut t_hw_0 = 0i64;
                let mut t_hw_1 = 0i64;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_hw_0);
                }
                let hwnd_hints = enumerate_raw_hwnd(hwnd, opts)?;
                unsafe {
                    let _ = QueryPerformanceCounter(&mut t_hw_1);
                }
                budget_warn(
                    "hwnd",
                    qpc_delta_ms(freq, t_hw_0, t_hw_1),
                    opts.budget_hwnd_ms,
                );
                Ok(NavEnumerateResult {
                    hints: hwnd_hints,
                    debug_rejects: r.debug_rejects,
                    timings_ms: None,
                })
            }
        }
    }

    /// Dump a bounded UIA subtree for troubleshooting (tray **Diagnose**).
    pub fn diagnose_uia_snapshot(
        &self,
        hwnd: UiaHwnd,
        max_depth: usize,
        max_nodes: usize,
    ) -> Result<String, UiaError> {
        crate::diagnose::snapshot_uia_tree(&self.automation, hwnd, max_depth, max_nodes)
    }

    /// Pattern dispatch: `Invoke` on the element located at the same `FindAll` index as enumeration.
    ///
    /// `opts` must match the [`EnumOptions`] used for the preceding [`UiaRuntime::enumerate`] call
    /// so descendant filtering stays consistent with `element_id`.
    pub fn invoke(&self, hwnd: UiaHwnd, hint: &Hint, opts: &EnumOptions) -> Result<(), UiaError> {
        match hint.raw.backend {
            Backend::Uia => {
                let find_cond = self.find_descendants_condition(opts)?;
                invoke_invoke_pattern(
                    &self.automation,
                    hwnd,
                    hint,
                    &self.invoke_find_cache,
                    &find_cond,
                )
            }
            Backend::Msaa => {
                eprintln!("[invoke] hint={} backend=MSAA", hint.label);
                unsafe { invoke_msaa_at(hwnd, hint.raw.element_id, GetForegroundWindow(), opts) }
            }
            Backend::RawHwnd => {
                eprintln!(
                    "[invoke] hint={} backend=RawHwnd fallback=SendInputClick",
                    hint.label
                );
                invoke_click_hint(&hint.raw)
            }
        }
    }
}

impl Drop for UiaRuntime {
    fn drop(&mut self) {
        if self.co_uninit_on_drop {
            unsafe { CoUninitialize() };
        }
    }
}
