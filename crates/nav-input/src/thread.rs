//! Message-only window, `WM_HOTKEY` dispatch, low-level keyboard hook for hint mode, and pump.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Console::GetConsoleWindow;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_A, VK_BACK,
    VK_ESCAPE, VK_OEM_1, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
    GetForegroundWindow, GetMessageW, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetForegroundWindow, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, UnregisterClassW, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_SYSKEYDOWN, WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::hotkey::PRIMARY_HOTKEY_ID;
use crate::{HotkeyPress, InputError, InputEvent, SessionKey};

const BRING_FOREGROUND_WPARAM: usize = 1;
/// `LLKHF_ALTDOWN` — pass through Alt combinations except the registered hotkey chord.
const LLKHF_ALTDOWN: u32 = 0x20;

struct PumpCtx {
    tx: Sender<InputEvent>,
    qpc_freq: i64,
}

static PUMP_CTX: OnceLock<PumpCtx> = OnceLock::new();

struct HookState {
    tx: Sender<InputEvent>,
    hint_mode: Arc<AtomicBool>,
}

static HOOK_STATE: OnceLock<HookState> = OnceLock::new();

/// Wide class name; must match [`crate::hotkey::MESSAGE_WINDOW_CLASS`](crate::hotkey::MESSAGE_WINDOW_CLASS).
fn class_pcwstr() -> PCWSTR {
    windows::core::w!("Navigator.InputSink.M2")
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
            });
            let _ = ctx.tx.send(event);
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
    if !state.hint_mode.load(Ordering::Acquire) {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let msg = wparam.0 as u32;
    if msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let vk = kb.vkCode;
    let flags = kb.flags.0;

    // Let `RegisterHotKey` (`Alt+;`) reach the message pump.
    if vk == VK_OEM_1.0 as u32 && (flags & LLKHF_ALTDOWN) != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    // Other Alt chords (menus, shortcuts) while hint mode is on.
    if (flags & LLKHF_ALTDOWN) != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let event = match vk {
        x if x == VK_ESCAPE.0 as u32 => Some(SessionKey::Escape),
        x if x == VK_BACK.0 as u32 => Some(SessionKey::Backspace),
        x if (VK_A.0 as u32..=VK_Z.0 as u32).contains(&x) => Some(SessionKey::Char(
            char::from_u32(x - VK_A.0 as u32 + u32::from(b'a')).unwrap_or('a'),
        )),
        _ => None,
    };

    if let Some(sk) = event {
        let _ = state.tx.send(InputEvent::SessionKey(sk));
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
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
}

impl InputThread {
    pub fn spawn() -> Result<(Self, crossbeam_channel::Receiver<InputEvent>), InputError> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (started_tx, started_rx) = mpsc::channel::<Result<(), InputError>>();
        let hint_mode = Arc::new(AtomicBool::new(false));
        let hint_for_thread = hint_mode.clone();

        let join = std::thread::spawn(move || {
            let setup = || -> Result<(HWND, HHOOK), InputError> {
                let mut freq = 0i64;
                unsafe { QueryPerformanceFrequency(&mut freq)? };

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
                });

                let hook = unsafe {
                    SetWindowsHookExW(
                        WH_KEYBOARD_LL,
                        Some(low_level_keyboard_proc),
                        Some(instance),
                        0,
                    )?
                };

                let mods = HOT_KEY_MODIFIERS(MOD_ALT.0 | MOD_NOREPEAT.0);
                let vk = VK_OEM_1.0 as u32;
                if let Err(e) = unsafe { RegisterHotKey(Some(hwnd), PRIMARY_HOTKEY_ID, mods, vk) } {
                    unsafe {
                        let _ = UnhookWindowsHookEx(hook);
                        let _ = DestroyWindow(hwnd);
                    }
                    return Err(InputError::HotkeyRegisterFailed {
                        details: e.to_string(),
                    });
                }

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
                },
                rx,
            )),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(InputError::ThreadEndedDuringStartup),
        }
    }
}
