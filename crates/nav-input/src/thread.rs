//! Message-only window, `WM_HOTKEY` dispatch, low-level keyboard hook for hint mode, and pump.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey, VK_A, VK_BACK,
    VK_CONTROL, VK_DIVIDE, VK_ESCAPE, VK_LWIN, VK_MENU, VK_OEM_2, VK_RWIN, VK_SHIFT, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
    GetForegroundWindow, GetMessageW, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetForegroundWindow, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, UnregisterClassW, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER, WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::chord;
use crate::focus;
use crate::hotkey::PRIMARY_HOTKEY_ID;

#[inline]
fn chord_is_plain_slash_only(raw: &str) -> bool {
    crate::hotkey_chord_is_plain_slash_only(raw)
}

static PLAIN_SLASH_ONLY_MODE: AtomicBool = AtomicBool::new(false);
use crate::{HotkeyPress, InputError, InputEvent, SessionKey};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

const BRING_FOREGROUND_WPARAM: usize = 1;
/// `LLKHF_ALTDOWN` — distinguish Alt-derived system key events in the LL hook.
const LLKHF_ALTDOWN: u32 = 0x20;
/// Injected key (ignore for plain-`/` activator).
const LLKHF_INJECTED: u32 = 0x10;

const WM_REREGISTER_HOTKEY: u32 = WM_USER + 302;

#[inline]
fn is_plain_slash_vk(vk: u32) -> bool {
    vk == VK_OEM_2.0 as u32 || vk == VK_DIVIDE.0 as u32
}

/// After we swallow plain `/` keydown, swallow repeats and matching keyup so the target app never sees `/`.
static PLAIN_SLASH_AWAITING_KEYUP: AtomicBool = AtomicBool::new(false);

static HOTKEY_ATOMIC: AtomicU64 = AtomicU64::new(0);
static INPUT_HWND: OnceLock<usize> = OnceLock::new();

#[inline]
fn store_hotkey(mods: HOT_KEY_MODIFIERS, vk: u32) {
    HOTKEY_ATOMIC.store(((mods.0 as u64) << 32) | vk as u64, Ordering::Release);
}

fn load_hotkey() -> (HOT_KEY_MODIFIERS, u32) {
    let v = HOTKEY_ATOMIC.load(Ordering::Acquire);
    (
        HOT_KEY_MODIFIERS((v >> 32) as u32),
        (v & 0xFFFF_FFFF) as u32,
    )
}

struct PumpCtx {
    tx: Sender<InputEvent>,
    qpc_freq: i64,
}

static PUMP_CTX: OnceLock<PumpCtx> = OnceLock::new();

struct HookState {
    tx: Sender<InputEvent>,
    hint_mode: Arc<AtomicBool>,
    /// When true with `hint_mode`, keys go to the focused app except Esc (which closes hints).
    keyboard_passthrough: Arc<AtomicBool>,
    /// Same QPC frequency as [`PumpCtx`] so plain-`/` dispatch never depends on `PUMP_CTX`.
    qpc_freq: i64,
}

static HOOK_STATE: OnceLock<HookState> = OnceLock::new();

/// Wide class name; must match [`crate::hotkey::MESSAGE_WINDOW_CLASS`](crate::hotkey::MESSAGE_WINDOW_CLASS).
fn class_pcwstr() -> PCWSTR {
    windows::core::w!("Navigator.InputSink.M2")
}

fn try_reregister_hotkey(hwnd: HWND, new_mods: HOT_KEY_MODIFIERS, new_vk: u32) {
    let (old_mods, old_vk) = load_hotkey();
    let _ = unsafe { UnregisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID) };
    let slash_only = PLAIN_SLASH_ONLY_MODE.load(Ordering::Acquire);
    if slash_only {
        store_hotkey(new_mods, new_vk);
        return;
    }
    if let Err(e) = unsafe { RegisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID, new_mods, new_vk) } {
        eprintln!("[input] hotkey reload failed: {e}. Restoring previous registration.");
        if unsafe { RegisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID, old_mods, old_vk) }.is_err() {
            eprintln!("[input] could not restore hotkey; you may need to restart Navigator.");
        }
    } else {
        store_hotkey(new_mods, new_vk);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let Some(ctx) = PUMP_CTX.get() else {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            };
            let mut t0 = 0i64;
            let _ = unsafe { QueryPerformanceCounter(&mut t0) };
            let fg = unsafe { GetForegroundWindow() };
            let mut t1 = 0i64;
            let _ = unsafe { QueryPerformanceCounter(&mut t1) };
            let us = qpc_delta_to_micros(ctx.qpc_freq, t0, t1);
            let event = InputEvent::Hotkey(HotkeyPress {
                id: wparam.0 as i32,
                captured_hwnd: fg.0 as usize,
                latency_us: us,
                from_plain_slash: false,
            });
            let _ = ctx.tx.send(event);
            LRESULT(0)
        }
        m if m == WM_REREGISTER_HOTKEY => {
            let new_mods = HOT_KEY_MODIFIERS(wparam.0 as u32);
            let new_vk = lparam.0 as u32;
            try_reregister_hotkey(hwnd, new_mods, new_vk);
            LRESULT(0)
        }
        m if m == WM_APP && wparam.0 == BRING_FOREGROUND_WPARAM => {
            let console = unsafe { GetConsoleWindow() };
            if !console.is_invalid() {
                let _ = unsafe { SetForegroundWindow(console) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// `reg` includes `MOD_NOREPEAT`; we mask that for key-state checks.
fn hotkey_mods_satisfied(reg: u32) -> bool {
    let r = reg & !(MOD_NOREPEAT.0);
    unsafe {
        if (r & MOD_ALT.0) != 0 && (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) == 0 {
            return false;
        }
        if (r & MOD_CONTROL.0) != 0 && (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) == 0
        {
            return false;
        }
        if (r & MOD_SHIFT.0) != 0 && (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) == 0 {
            return false;
        }
        if (r & MOD_WIN.0) != 0 {
            let win = (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0;
            if !win {
                return false;
            }
        }
    }
    true
}

fn modifiers_preclude_plain_slash_activator() -> bool {
    unsafe {
        (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
    }
}

fn dispatch_plain_slash_hotkey(state: &HookState) {
    let mut t0 = 0i64;
    let _ = unsafe { QueryPerformanceCounter(&mut t0) };
    let fg = unsafe { GetForegroundWindow() };
    let mut t1 = 0i64;
    let _ = unsafe { QueryPerformanceCounter(&mut t1) };
    let us = qpc_delta_to_micros(state.qpc_freq, t0, t1);
    let event = InputEvent::Hotkey(HotkeyPress {
        id: PRIMARY_HOTKEY_ID,
        captured_hwnd: fg.0 as usize,
        latency_us: us,
        from_plain_slash: true,
    });
    let _ = state.tx.send(event);
}

unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    let Some(state) = HOOK_STATE.get() else {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    };

    let msg = wparam.0 as u32;
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let vk = kb.vkCode;
    let flags = kb.flags.0;

    if msg == WM_KEYUP || msg == WM_SYSKEYUP {
        if is_plain_slash_vk(vk) && PLAIN_SLASH_AWAITING_KEYUP.load(Ordering::Acquire) {
            PLAIN_SLASH_AWAITING_KEYUP.store(false, Ordering::Release);
            return LRESULT(1);
        }
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    if msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    if state.hint_mode.load(Ordering::Acquire) {
        if state.keyboard_passthrough.load(Ordering::Acquire) {
            if vk == VK_ESCAPE.0 as u32 {
                let _ = state.tx.send(InputEvent::SessionKey(SessionKey::Escape));
                return LRESULT(1);
            }
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let (reg_mods, reg_vk) = load_hotkey();
        // Plain `/` chord: `/` must not reach the focused app while hints are up (second `/` → editing).
        if PLAIN_SLASH_ONLY_MODE.load(Ordering::Acquire)
            && is_plain_slash_vk(vk)
            && (flags & LLKHF_INJECTED) == 0
            && !modifiers_preclude_plain_slash_activator()
        {
            dispatch_plain_slash_hotkey(state);
            return LRESULT(1);
        }
        if vk == reg_vk && hotkey_mods_satisfied(reg_mods.0) {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        if (flags & LLKHF_ALTDOWN) != 0 {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        if (unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } as u16 & 0x8000) != 0
            || (unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) } as u16 & 0x8000) != 0
            || (unsafe { GetAsyncKeyState(VK_RWIN.0 as i32) } as u16 & 0x8000) != 0
        {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }

        let event = match vk {
            x if x == VK_ESCAPE.0 as u32 => Some(SessionKey::Escape),
            x if x == VK_BACK.0 as u32 => Some(SessionKey::Backspace),
            x if (VK_A.0 as u32..=VK_Z.0 as u32).contains(&x) => {
                chord::vk_session_char(x).map(SessionKey::Char)
            }
            _ => None,
        };

        return if let Some(sk) = event {
            let _ = state.tx.send(InputEvent::SessionKey(sk));
            LRESULT(1)
        } else {
            unsafe { CallNextHookEx(None, code, wparam, lparam) }
        };
    }

    if !is_plain_slash_vk(vk) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    if (flags & LLKHF_INJECTED) != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    if modifiers_preclude_plain_slash_activator() {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    if PLAIN_SLASH_AWAITING_KEYUP.load(Ordering::Acquire) {
        return LRESULT(1);
    }

    PLAIN_SLASH_AWAITING_KEYUP.store(true, Ordering::Release);
    dispatch_plain_slash_hotkey(state);
    LRESULT(1)
}

fn qpc_delta_to_micros(freq: i64, t0: i64, t1: i64) -> u64 {
    if freq <= 0 {
        return 0;
    }
    let dt = t1.saturating_sub(t0);
    let num = (dt as i128).saturating_mul(1_000_000);
    (num / freq as i128).max(0) as u64
}

pub(super) fn poke_peer_for_foreground() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(class_pcwstr(), PCWSTR::null()) {
            if !hwnd.is_invalid() {
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_APP,
                    WPARAM(BRING_FOREGROUND_WPARAM),
                    LPARAM(0),
                );
            }
        }
    }
}

pub struct InputThread {
    _join: JoinHandle<()>,
    pub hint_mode: Arc<AtomicBool>,
    pub keyboard_passthrough: Arc<AtomicBool>,
}

impl InputThread {
    /// Spawns the input thread and registers a global hotkey from `chord` (see `nav-config` `[hotkey].chord`).
    pub fn spawn_with_chord(
        chord: &str,
    ) -> Result<(Self, crossbeam_channel::Receiver<InputEvent>), InputError> {
        let slash_only = chord_is_plain_slash_only(chord);
        PLAIN_SLASH_ONLY_MODE.store(slash_only, Ordering::Release);
        let (init_mods, init_vk) = if slash_only {
            (HOT_KEY_MODIFIERS(MOD_NOREPEAT.0), VK_OEM_2.0 as u32)
        } else {
            chord::parse_chord(chord)
                .map_err(|e| InputError::HotkeyRegisterFailed { details: e })?
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        let (started_tx, started_rx) = mpsc::channel::<Result<(), InputError>>();
        let hint_mode = Arc::new(AtomicBool::new(false));
        let hint_for_thread = hint_mode.clone();
        let keyboard_passthrough = Arc::new(AtomicBool::new(false));
        let passthrough_for_thread = keyboard_passthrough.clone();

        let join = std::thread::spawn(move || {
            let setup = || -> Result<(HWND, HHOOK), InputError> {
                let mut freq = 0i64;
                unsafe { QueryPerformanceFrequency(&mut freq)? };

                focus::ensure_com_apartment();

                let module = unsafe { GetModuleHandleW(None)? };
                let instance = HINSTANCE(module.0);

                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance,
                    lpszClassName: class_pcwstr(),
                    ..Default::default()
                };
                let atom = unsafe { RegisterClassExW(&wc) };
                if atom == 0 {
                    use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError};
                    let err = unsafe { GetLastError() };
                    if err != ERROR_CLASS_ALREADY_EXISTS {
                        return Err(InputError::HotkeyRegisterFailed {
                            details: format!(
                                "RegisterClassExW failed (class may be invalid); GetLastError={err:?}"
                            ),
                        });
                    }
                }

                let hwnd = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class_pcwstr(),
                        PCWSTR::null(),
                        WINDOW_STYLE::default(),
                        0,
                        0,
                        0,
                        0,
                        Some(HWND_MESSAGE),
                        None,
                        Some(instance),
                        None,
                    )?
                };

                let _ = PUMP_CTX.set(PumpCtx {
                    tx: tx.clone(),
                    qpc_freq: freq,
                });

                let _ = HOOK_STATE.set(HookState {
                    tx: tx.clone(),
                    hint_mode: hint_for_thread,
                    keyboard_passthrough: passthrough_for_thread,
                    qpc_freq: freq,
                });

                let hook = unsafe {
                    SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(low_level_keyboard_proc),
                        Some(instance),
                        0,
                    )?
                };

                if !slash_only {
                    if let Err(e) =
                        unsafe { RegisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID, init_mods, init_vk) }
                    {
                        unsafe {
                            let _ = UnhookWindowsHookEx(hook);
                            let _ = DestroyWindow(hwnd);
                        }
                        return Err(InputError::HotkeyRegisterFailed {
                            details: e.to_string(),
                        });
                    }
                }
                store_hotkey(init_mods, init_vk);
                let _ = INPUT_HWND.set(hwnd.0 as usize);

                Ok((hwnd, hook))
            };

            match setup() {
                Ok((hwnd, hook)) => {
                    if started_tx.send(Ok(())).is_err() {
                        unsafe {
                            let _ = UnregisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID);
                            let _ = UnhookWindowsHookEx(hook);
                            let _ = DestroyWindow(hwnd);
                        }
                        return;
                    }

                    let mut msg = MSG::default();
                    loop {
                        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                        if !ret.as_bool() {
                            break;
                        }
                        unsafe {
                            let _ = TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }

                    unsafe {
                        let _ = UnhookWindowsHookEx(hook);
                    }

                    let module = unsafe { GetModuleHandleW(None) };
                    if let Ok(module) = module {
                        let instance = HINSTANCE(module.0);
                        unsafe {
                            let _ = UnregisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID);
                            let _ = DestroyWindow(hwnd);
                            let _ = UnregisterClassW(class_pcwstr(), Some(instance));
                        }
                    }
                }
                Err(e) => {
                    let _ = started_tx.send(Err(e));
                }
            }
        });

        match started_rx.recv() {
            Ok(Ok(())) => Ok((
                InputThread {
                    _join: join,
                    hint_mode,
                    keyboard_passthrough,
                },
                rx,
            )),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(InputError::ThreadEndedDuringStartup),
        }
    }

    /// Re-parses `chord` and re-registers the hotkey on the input thread (e.g. after tray Reload).
    pub fn reregister_hotkey(&self, chord: &str) -> Result<(), InputError> {
        let slash_only = chord_is_plain_slash_only(chord);
        PLAIN_SLASH_ONLY_MODE.store(slash_only, Ordering::Release);
        let (mods, vk) = if slash_only {
            (HOT_KEY_MODIFIERS(MOD_NOREPEAT.0), VK_OEM_2.0 as u32)
        } else {
            chord::parse_chord(chord)
                .map_err(|e| InputError::HotkeyRegisterFailed { details: e })?
        };
        let Some(&raw) = INPUT_HWND.get() else {
            return Err(InputError::ThreadEndedDuringStartup);
        };
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_REREGISTER_HOTKEY,
                WPARAM(mods.0 as usize),
                LPARAM(vk as isize),
            )?;
        }
        Ok(())
    }
}
