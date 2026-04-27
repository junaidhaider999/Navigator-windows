//! MSAA (`IAccessible`) enumeration when UIA yields no invoke targets (Phase E1).

use std::mem::ManuallyDrop;

use nav_core::{Backend, ElementKind, RawHint, Rect};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::Win32::UI::Accessibility::{AccessibleObjectFromWindow, IAccessible};
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible, OBJID_CLIENT};
use windows::core::Interface;

use crate::UiaError;
use crate::hwnd::UiaHwnd;
use crate::options::EnumOptions;

const ROLE_PUSHBUTTON: i32 = 43;
const ROLE_RADIOBUTTON: i32 = 45;
const ROLE_CHECKBUTTON: i32 = 44;
const ROLE_COMBOBOX: i32 = 46;
const ROLE_LISTITEM: i32 = 34;
const ROLE_MENUITEM: i32 = 12;
const ROLE_LINK: i32 = 30;
const ROLE_STATICTEXT: i32 = 41;
const ROLE_GRAPHIC: i32 = 40;

const STATE_INVISIBLE: i32 = 0x8000;
const STATE_OFFSCREEN: i32 = 0x10000;

fn variant_i4(n: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: n },
            }),
        },
    }
}

fn i32_from_variant(v: &VARIANT) -> Option<i32> {
    let inner = unsafe { &*v.Anonymous.Anonymous };
    if inner.vt != VT_I4 {
        return None;
    }
    Some(unsafe { inner.Anonymous.lVal })
}

unsafe fn try_root_accessible(hwnd: UiaHwnd) -> Result<IAccessible, UiaError> {
    let mut p: *mut std::ffi::c_void = std::ptr::null_mut();
    unsafe {
        AccessibleObjectFromWindow(hwnd, OBJID_CLIENT.0 as u32, &IAccessible::IID, &mut p)
            .map_err(|e| UiaError::Operation(format!("AccessibleObjectFromWindow: {e}")))?;
        if p.is_null() {
            return Err(UiaError::Operation(
                "AccessibleObjectFromWindow returned null".into(),
            ));
        }
        Ok(IAccessible::from_raw(p.cast()))
    }
}

fn role_to_kind(role: i32) -> Option<ElementKind> {
    match role {
        ROLE_PUSHBUTTON | ROLE_RADIOBUTTON | ROLE_CHECKBUTTON | ROLE_MENUITEM | ROLE_LISTITEM => {
            Some(ElementKind::Invoke)
        }
        ROLE_COMBOBOX => Some(ElementKind::Editable),
        ROLE_LINK => Some(ElementKind::Invoke),
        ROLE_STATICTEXT | ROLE_GRAPHIC => None,
        _ => Some(ElementKind::GenericClickable),
    }
}

fn should_take_node(role: i32, state: i32, opts: &EnumOptions, width: i32, height: i32) -> bool {
    if (state & 1) != 0 {
        return false;
    }
    if !opts.include_offscreen && (state & STATE_OFFSCREEN) != 0 {
        return false;
    }
    if !opts.include_offscreen && (state & STATE_INVISIBLE) != 0 {
        return false;
    }
    if width < 2 || height < 2 {
        return false;
    }
    if matches!(role, ROLE_STATICTEXT | ROLE_GRAPHIC) && (width * height) > 400 * 400 {
        return false;
    }
    role_to_kind(role).is_some()
}

unsafe fn acc_location(acc: &IAccessible) -> Option<Rect> {
    let v = variant_i4(0);
    let mut left = 0i32;
    let mut top = 0i32;
    let mut w = 0i32;
    let mut h = 0i32;
    unsafe { acc.accLocation(&mut left, &mut top, &mut w, &mut h, &v) }.ok()?;
    if w < 1 || h < 1 {
        return None;
    }
    Some(Rect {
        x: left,
        y: top,
        w,
        h,
    })
}

unsafe fn walk_collect(
    acc: &IAccessible,
    opts: &EnumOptions,
    max: usize,
    out: &mut Vec<RawHint>,
) -> Result<(), UiaError> {
    if out.len() >= max {
        return Ok(());
    }

    let v0 = variant_i4(0);
    let role = unsafe { acc.get_accRole(&v0) }
        .ok()
        .as_ref()
        .and_then(i32_from_variant)
        .unwrap_or(-1);
    let state = unsafe { acc.get_accState(&v0) }
        .ok()
        .as_ref()
        .and_then(i32_from_variant)
        .unwrap_or(0);

    if let Some(loc) = unsafe { acc_location(acc) } {
        if should_take_node(role, state, opts, loc.w, loc.h) {
            let kind = role_to_kind(role).unwrap_or(ElementKind::GenericClickable);
            out.push(RawHint {
                element_id: out.len() as u64,
                uia_runtime_id_fp: None,
                uia_invoke_hwnd: None,
                uia_child_index: None,
                bounds: loc,
                kind,
                name: None,
                backend: Backend::Msaa,
            });
        }
    }

    let count = unsafe { acc.accChildCount() }.unwrap_or(0);
    if count <= 0 || out.len() >= max {
        return Ok(());
    }

    for i in 1..=count {
        if out.len() >= max {
            break;
        }
        let cv = variant_i4(i);
        let child_disp = match unsafe { acc.get_accChild(&cv) } {
            Ok(d) => d,
            Err(_) => continue,
        };
        let child_acc: IAccessible = match child_disp.cast() {
            Ok(x) => x,
            Err(_) => continue,
        };
        unsafe { walk_collect(&child_acc, opts, max, out) }?;
    }
    Ok(())
}

/// MSAA enumeration for `hwnd` (session root).
pub fn enumerate_msaa(hwnd: UiaHwnd, opts: &EnumOptions) -> Result<Vec<RawHint>, UiaError> {
    if hwnd.is_invalid() {
        return Ok(Vec::new());
    }
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || !unsafe { IsWindowEnabled(hwnd) }.as_bool() {
        return Ok(Vec::new());
    }
    let acc = unsafe { try_root_accessible(hwnd)? };
    let mut out = Vec::new();
    unsafe { walk_collect(&acc, opts, opts.max_elements, &mut out) }?;
    Ok(out)
}

unsafe fn invoke_walk(
    acc: &IAccessible,
    opts: &EnumOptions,
    target: u64,
    cur: &mut u64,
    foreground: windows::Win32::Foundation::HWND,
) -> Result<bool, UiaError> {
    if unsafe { GetForegroundWindow() } != foreground {
        return Err(UiaError::Operation(
            "foreground changed before MSAA invoke".into(),
        ));
    }

    let v0 = variant_i4(0);
    let role = unsafe { acc.get_accRole(&v0) }
        .ok()
        .as_ref()
        .and_then(i32_from_variant)
        .unwrap_or(-1);
    let state = unsafe { acc.get_accState(&v0) }
        .ok()
        .as_ref()
        .and_then(i32_from_variant)
        .unwrap_or(0);

    if let Some(loc) = unsafe { acc_location(acc) } {
        if should_take_node(role, state, opts, loc.w, loc.h) {
            if *cur == target {
                unsafe { acc.accDoDefaultAction(&v0) }
                    .map_err(|e| UiaError::Operation(format!("accDoDefaultAction: {e}")))?;
                return Ok(true);
            }
            *cur += 1;
        }
    }

    let count = unsafe { acc.accChildCount() }.unwrap_or(0);
    for i in 1..=count {
        let cv = variant_i4(i);
        let child_disp = match unsafe { acc.get_accChild(&cv) } {
            Ok(d) => d,
            Err(_) => continue,
        };
        let child_acc: IAccessible = match child_disp.cast() {
            Ok(x) => x,
            Err(_) => continue,
        };
        if unsafe { invoke_walk(&child_acc, opts, target, cur, foreground) }? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `element_id` matches the index produced by [`enumerate_msaa`].
pub unsafe fn invoke_msaa_at(
    hwnd: UiaHwnd,
    element_id: u64,
    foreground: windows::Win32::Foundation::HWND,
    opts: &EnumOptions,
) -> Result<(), UiaError> {
    if hwnd.is_invalid() {
        return Err(UiaError::Operation("invalid HWND for MSAA invoke".into()));
    }
    let acc = unsafe { try_root_accessible(hwnd)? };
    let mut cur = 0u64;
    if unsafe { invoke_walk(&acc, opts, element_id, &mut cur, foreground)? } {
        return Ok(());
    }
    Err(UiaError::Operation(
        "MSAA invoke: element_id not matched in walk".into(),
    ))
}
