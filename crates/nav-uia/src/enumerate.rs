//! Slow baseline: `ElementFromHandle` + `FindAll(TreeScope_Descendants, true)` + per-element reads.

use crate::hwnd::UiaHwnd;
use nav_core::{Backend, ElementKind, RawHint};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement, TreeScope_Descendants,
};
use windows::core::BSTR;

use crate::UiaError;
use crate::coords::rect_from_uia_bounds;
use crate::options::EnumOptions;
use crate::pattern::has_invoke_pattern_cached;

/// Cached enumeration: `FindAllBuildCache` + invoke / bounds / enabled filters.
pub fn enumerate_baseline(
    automation: &IUIAutomation,
    hwnd: UiaHwnd,
    opts: &EnumOptions,
    cache: &IUIAutomationCacheRequest,
) -> Result<Vec<RawHint>, UiaError> {
    if hwnd.is_invalid() {
        return Ok(Vec::new());
    }

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let true_cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| UiaError::Operation(e.to_string()))?;

    let all = unsafe { root.FindAllBuildCache(TreeScope_Descendants, &true_cond, cache) }
        .map_err(|e| UiaError::Operation(format!("FindAllBuildCache: {e}")))?;

    let len = unsafe { all.Length() }.map_err(|e| UiaError::Operation(e.to_string()))?;

    let mut out = Vec::new();

    for i in 0..len {
        if out.len() >= opts.max_elements {
            break;
        }

        let el = match unsafe { all.GetElement(i) } {
            Ok(e) => e,
            Err(e) => return Err(UiaError::Operation(e.to_string())),
        };

        if !has_invoke_pattern_cached(&el) {
            continue;
        }

        if !opts.include_disabled {
            match unsafe { el.CurrentIsEnabled() } {
                Ok(b) if !b.as_bool() => continue,
                Err(_) => continue,
                _ => {}
            }
        }

        if !opts.include_offscreen {
            match unsafe { el.CurrentIsOffscreen() } {
                Ok(b) if b.as_bool() => continue,
                Err(_) => {}
                _ => {}
            }
        }

        let rect = match unsafe { el.CurrentBoundingRectangle() } {
            Ok(r) => match rect_from_uia_bounds(r) {
                Some(r) => r,
                None => continue,
            },
            Err(_) => continue,
        };

        let name = read_optional_name(&el);

        out.push(RawHint {
            element_id: i as u64,
            bounds: rect,
            kind: ElementKind::Invoke,
            name,
            backend: Backend::Uia,
        });
    }

    Ok(out)
}

fn read_optional_name(el: &IUIAutomationElement) -> Option<Box<str>> {
    let bstr: BSTR = unsafe { el.CurrentName() }.ok()?;
    let s = bstr.to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.into_boxed_str())
    }
}
