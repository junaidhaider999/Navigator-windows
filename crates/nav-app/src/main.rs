#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("Navigator requires Windows.");
    std::process::ExitCode::from(1)
}

#[cfg(windows)]
mod logging;
#[cfg(windows)]
mod single_instance;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::sync::atomic::Ordering;

    use clap::Parser;
    use nav_core::{Session, SessionEvent, plan};
    use nav_input::{InputEvent, InputThread, SessionKey};
    use nav_render::Renderer;
    use nav_uia::{EnumOptions, UiaRuntime};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    #[derive(Parser)]
    #[command(name = "navigator", about = "Navigator — keyboard-native UI hints")]
    struct Cli {
        /// Enable `tracing` to stderr (e.g. trace, debug, info, warn, error).
        #[arg(long, value_name = "LEVEL")]
        log: Option<String>,
    }

    let cli = Cli::parse();
    logging::init(cli.log.as_deref());

    let _guard = match single_instance::acquire() {
        Ok(g) => g,
        Err(single_instance::Error::AlreadyRunning) => {
            nav_input::poke_peer_for_foreground();
            return std::process::ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("single-instance: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    let (input, rx) = match InputThread::spawn() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(1);
        }
    };

    let uia = match UiaRuntime::new() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("uia init: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let renderer = match Renderer::spawn() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("render init: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    if let Err(e) = renderer.prewarm() {
        eprintln!("render prewarm: {e}");
    }
    let enum_opts = EnumOptions::default();
    let mut overlay_session: u64 = 0;
    let mut active_show_id: Option<u64> = None;
    let mut session: Option<Session> = None;
    let mut session_hwnd: Option<HWND> = None;
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();

    println!("Navigator ready");

    while let Ok(ev) = rx.recv() {
        match ev {
            InputEvent::Hotkey(p) => {
                println!(
                    "[input] hotkey captured_hwnd=0x{:x} latency_us={}",
                    p.captured_hwnd, p.latency_us
                );

                if input.hint_mode.load(Ordering::Acquire) {
                    session = None;
                    session_hwnd = None;
                    input.hint_mode.store(false, Ordering::Release);
                    if let Some(sid) = active_show_id.take() {
                        let _ = renderer.hide(sid);
                    }
                }

                if p.captured_hwnd == 0 {
                    eprintln!("[uia] skipped: null foreground hwnd snapshot");
                    continue;
                }

                let hwnd = HWND(p.captured_hwnd as *mut core::ffi::c_void);

                overlay_session = overlay_session.wrapping_add(1);
                if let Some(prev) = active_show_id {
                    let _ = renderer.hide(prev);
                }

                let mut freq = 0i64;
                if unsafe { QueryPerformanceFrequency(&mut freq) }.is_err() {
                    eprintln!("[uia] QueryPerformanceFrequency failed");
                    continue;
                }

                let mut t0 = 0i64;
                if unsafe { QueryPerformanceCounter(&mut t0) }.is_err() {
                    eprintln!("[uia] QueryPerformanceCounter failed");
                    continue;
                }

                let enum_res = uia.enumerate(hwnd, &enum_opts);

                let mut t1 = 0i64;
                if unsafe { QueryPerformanceCounter(&mut t1) }.is_err() {
                    eprintln!("[uia] QueryPerformanceCounter (end) failed");
                    continue;
                }

                let took_ms = if freq > 0 {
                    (t1.saturating_sub(t0) as f64) * 1000.0 / freq as f64
                } else {
                    0.0
                };

                let raws = match enum_res {
                    Ok(elements) => {
                        println!(
                            "[uia] hwnd=0x{:x} elements={} took_ms={:.2}",
                            p.captured_hwnd,
                            elements.len(),
                            took_ms
                        );
                        elements
                    }
                    Err(e) => {
                        eprintln!("[uia] error: {e}");
                        continue;
                    }
                };

                let mut wr = windows::Win32::Foundation::RECT::default();
                let layout_origin = if unsafe { GetWindowRect(hwnd, &mut wr) }.is_ok() {
                    nav_core::Rect {
                        x: wr.left,
                        y: wr.top,
                        w: (wr.right - wr.left).max(1),
                        h: (wr.bottom - wr.top).max(1),
                    }
                } else {
                    nav_core::Rect {
                        x: 0,
                        y: 0,
                        w: 1,
                        h: 1,
                    }
                };

                let hints = plan(raws, &alphabet, layout_origin);
                let mut sess = Session::new(overlay_session);
                sess.ingest(hints);
                let initial = sess.visible_hints();

                if initial.is_empty() {
                    session = None;
                    session_hwnd = None;
                    active_show_id = None;
                    continue;
                }

                if let Err(e) = renderer.show(overlay_session, &initial) {
                    eprintln!("[render] show: {e}");
                    session = None;
                    session_hwnd = None;
                    active_show_id = None;
                    continue;
                }

                active_show_id = Some(overlay_session);
                session = Some(sess);
                session_hwnd = Some(hwnd);
                input.hint_mode.store(true, Ordering::Release);
            }
            InputEvent::SessionKey(sk) => {
                if !input.hint_mode.load(Ordering::Acquire) {
                    continue;
                }
                let Some(sid) = active_show_id else {
                    input.hint_mode.store(false, Ordering::Release);
                    continue;
                };
                let Some(mut sess) = session.take() else {
                    input.hint_mode.store(false, Ordering::Release);
                    let _ = renderer.hide(sid);
                    active_show_id = None;
                    session_hwnd = None;
                    continue;
                };

                let event = match sk {
                    SessionKey::Escape => sess.cancel(),
                    SessionKey::Backspace => sess.key('\u{8}'),
                    SessionKey::Char(c) => sess.key(c),
                };

                match event {
                    SessionEvent::Render(_) => {
                        let visible = sess.visible_hints();
                        if let Err(e) = renderer.repaint(sid, &visible) {
                            eprintln!("[render] repaint: {e}");
                        }
                        session = Some(sess);
                    }
                    SessionEvent::Invoke(id) => {
                        let hwnd = session_hwnd.take();
                        let hint = sess.hints().get(id.0 as usize).cloned();
                        input.hint_mode.store(false, Ordering::Release);
                        let _ = renderer.hide(sid);
                        active_show_id = None;
                        if let (Some(hwnd), Some(h)) = (hwnd, hint) {
                            if let Err(e) = uia.invoke(hwnd, &h) {
                                eprintln!("[uia] invoke: {e}");
                            }
                        }
                    }
                    SessionEvent::Done => {
                        input.hint_mode.store(false, Ordering::Release);
                        let _ = renderer.hide(sid);
                        active_show_id = None;
                        session_hwnd = None;
                    }
                }
            }
        }
    }

    std::process::ExitCode::SUCCESS
}
