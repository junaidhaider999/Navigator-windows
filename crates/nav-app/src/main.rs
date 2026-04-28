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
mod tray;

#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
#[derive(clap::Parser)]
#[command(name = "navigator", about = "Navigator — keyboard-native UI hints")]
struct Cli {
    #[arg(long, value_name = "LEVEL")]
    log: Option<String>,
    #[arg(long)]
    debug_uia: bool,
    #[arg(long)]
    debug_overlay: bool,
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(long)]
    print_config: bool,
    #[arg(long)]
    reset_config: bool,
    #[arg(long, default_value_t = false)]
    no_tray: bool,
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::sync::{Arc, Mutex};

    use clap::Parser;
    use crossbeam_channel::select;
    use nav_input::InputThread;
    use nav_render::Renderer;
    use nav_uia::UiaRuntime;

    let cli = Cli::parse();

    if cli.reset_config {
        let path = cli
            .config
            .clone()
            .unwrap_or_else(nav_config::default_user_config_path);
        match nav_config::write_default_config(path.as_path()) {
            Ok(()) => {
                println!("Wrote default config to {}", path.display());
                return std::process::ExitCode::from(0);
            }
            Err(e) => {
                eprintln!("config: {e}");
                return std::process::ExitCode::from(1);
            }
        }
    }

    let cfg = match nav_config::load_for_startup(cli.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    if cli.print_config {
        println!("{cfg:#?}");
        return std::process::ExitCode::from(0);
    }
    let alphabet = nav_config::alphabet_chars(&cfg);
    if alphabet.len() < 2 {
        eprintln!("config: [hints].alphabet must have at least 2 non-whitespace characters");
        return std::process::ExitCode::from(1);
    }

    let cli_snap = CliSnapshot {
        debug_uia: cli.debug_uia,
        debug_overlay: cli.debug_overlay,
    };

    let log_effective = cli.log.clone().or(cfg.log.level.clone());
    logging::init(log_effective.as_deref());

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

    let (input, rx) = match InputThread::spawn_with_chord(&cfg.hotkey.chord) {
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

    let enum_opts = build_enum_opts(&cfg, &cli_snap);
    let app = Arc::new(Mutex::new(AppState {
        alphabet,
        enum_opts,
        hint_cache_ttl_ms: cfg.hints.hint_cache_ttl_ms,
        pipeline_soft_budget_ms: cfg.hints.pipeline_soft_budget_ms as f64,
        pipeline_hard_budget_ms: cfg.hints.pipeline_hard_budget_ms as f64,
        planner_label_cap: cfg.hints.planner_label_cap,
        hint_cache: None,
    }));
    let last_focus: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));

    let debug_pill_connectors = cli.debug_overlay;

    let mut l = LoopCtx {
        overlay_session: 0,
        active_show_id: None,
        session: None,
        session_hwnd: None,
        active_debug_rejects: Vec::new(),
        overlay_debug_only: false,
    };

    let (tray_tx, tray_rx) = crossbeam_channel::unbounded::<tray::TrayEvent>();
    if !cli.no_tray {
        tray::spawn(tray_tx);
    }

    println!("Navigator ready");

    let path_cli = cli.config.clone();

    let reload = || match nav_config::load_for_startup(path_cli.as_deref()) {
        Ok(c) => {
            let alph = nav_config::alphabet_chars(&c);
            if alph.len() < 2 {
                eprintln!("[config] reload: alphabet too short");
                return;
            }
            let opts = build_enum_opts(&c, &cli_snap);
            *app.lock().expect("state") = AppState {
                alphabet: alph,
                enum_opts: opts,
                hint_cache_ttl_ms: c.hints.hint_cache_ttl_ms,
                pipeline_soft_budget_ms: c.hints.pipeline_soft_budget_ms as f64,
                pipeline_hard_budget_ms: c.hints.pipeline_hard_budget_ms as f64,
                planner_label_cap: c.hints.planner_label_cap,
                hint_cache: None,
            };
            if let Err(e) = input.reregister_hotkey(&c.hotkey.chord) {
                eprintln!("[config] reload: hotkey: {e}");
            }
            if let Err(e) = renderer.sync_monitors() {
                eprintln!("[config] reload: overlay sync: {e}");
            }
            println!("[config] reloaded");
        }
        Err(e) => eprintln!("[config] reload failed: {e}"),
    };

    loop {
        if cli.no_tray {
            let Ok(ev) = rx.recv() else {
                break;
            };
            dispatch_input(
                ev,
                &mut l,
                &input,
                &uia,
                &renderer,
                &app,
                &last_focus,
                debug_pill_connectors,
            );
            continue;
        }

        select! {
            recv(rx) -> ev => {
                let Ok(ev) = ev else { break };
                dispatch_input(
                    ev,
                    &mut l,
                    &input,
                    &uia,
                    &renderer,
                    &app,
                    &last_focus,
                    debug_pill_connectors,
                );
            }
            recv(tray_rx) -> te => {
                let Ok(te) = te else { break };
                match te {
                    tray::TrayEvent::Reload => reload(),
                    tray::TrayEvent::OpenConfigFolder => open_config_folder(),
                    tray::TrayEvent::Diagnose => {
                        let hwnd = last_focus
                            .lock()
                            .expect("last_focus")
                            .map(|p| {
                                windows::Win32::Foundation::HWND(
                                    (p as isize) as *mut core::ffi::c_void,
                                )
                            })
                            .unwrap_or_else(|| unsafe {
                                windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()
                            });
                        match uia.diagnose_uia_snapshot(hwnd, 4, 260) {
                            Ok(text) => {
                                let mut path = std::env::temp_dir();
                                path.push("navigator-uia-dump.txt");
                                match std::fs::write(&path, &text) {
                                    Ok(()) => println!(
                                        "[diagnose] Wrote UIA snapshot (HWND=0x{:x}) to {}",
                                        hwnd.0 as usize,
                                        path.display()
                                    ),
                                    Err(e) => eprintln!("[diagnose] write failed: {e}"),
                                }
                            }
                            Err(e) => eprintln!("[diagnose] {e}"),
                        }
                    }
                    tray::TrayEvent::About => show_about_dialog(),
                    tray::TrayEvent::Quit => return std::process::ExitCode::SUCCESS,
                }
            }
        }
    }

    std::process::ExitCode::SUCCESS
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct CliSnapshot {
    debug_uia: bool,
    debug_overlay: bool,
}

#[cfg(windows)]
struct HintSessionCache {
    hwnd: usize,
    pid: u32,
    title_fp: u64,
    rect_ltrb: (i32, i32, i32, i32),
    raws_deduped: Vec<nav_core::RawHint>,
    debug_rejects: Vec<nav_core::UiaDebugReject>,
    at: std::time::Instant,
}

#[cfg(windows)]
struct AppState {
    alphabet: Vec<char>,
    enum_opts: nav_uia::EnumOptions,
    hint_cache_ttl_ms: u64,
    pipeline_soft_budget_ms: f64,
    pipeline_hard_budget_ms: f64,
    planner_label_cap: usize,
    hint_cache: Option<HintSessionCache>,
}

#[cfg(windows)]
fn parse_enumeration_profile(s: &str) -> nav_uia::EnumerationProfile {
    match s.trim().to_ascii_lowercase().as_str() {
        "full" => nav_uia::EnumerationProfile::Full,
        _ => nav_uia::EnumerationProfile::Fast,
    }
}

#[cfg(windows)]
fn parse_enumeration_ladder(s: &str) -> nav_uia::EnumerationStrategyMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "uia_first" | "uiafirst" | "uia" => nav_uia::EnumerationStrategyMode::UiaFirst,
        "win32_first" | "win32first" | "hwnd_first" | "win32" => {
            nav_uia::EnumerationStrategyMode::Win32First
        }
        "chromium_fast" | "chromium" | "electron" => nav_uia::EnumerationStrategyMode::ChromiumFast,
        _ => nav_uia::EnumerationStrategyMode::Auto,
    }
}

#[cfg(windows)]
fn build_enum_opts(cfg: &nav_config::Config, cli: &CliSnapshot) -> nav_uia::EnumOptions {
    nav_uia::EnumOptions {
        max_elements: cfg.hints.max_elements,
        profile: parse_enumeration_profile(&cfg.hints.enumeration_profile),
        materialize_hard_budget_ms: cfg.hints.materialize_budget_ms,
        strategy_mode: parse_enumeration_ladder(&cfg.hints.enumeration_ladder),
        budget_uia_ms: cfg.fallback.budget_ms.uia,
        budget_msaa_ms: cfg.fallback.budget_ms.msaa,
        budget_hwnd_ms: cfg.fallback.budget_ms.hwnd,
        debug_uia: cli.debug_uia,
        debug_overlay: cli.debug_overlay,
        ..Default::default()
    }
}

#[cfg(windows)]
struct LoopCtx {
    overlay_session: u64,
    active_show_id: Option<u64>,
    session: Option<nav_core::Session>,
    session_hwnd: Option<windows::Win32::Foundation::HWND>,
    active_debug_rejects: Vec<nav_core::UiaDebugReject>,
    overlay_debug_only: bool,
}

#[cfg(windows)]
fn open_config_folder() {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};

    let dir = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|p| p.join("Navigator"));
    let Some(d) = dir else {
        return;
    };
    let wide: Vec<u16> = d
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = ShellExecuteW(
            None,
            w!("explore"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(windows)]
fn show_about_dialog() {
    use windows::Win32::UI::WindowsAndMessaging::{
        MB_ICONINFORMATION, MB_OK, MESSAGEBOX_STYLE, MessageBoxW,
    };
    use windows::core::PCWSTR;

    let body = format!(
        "Navigator {}\nKeyboard-native UI hints for Windows.",
        env!("CARGO_PKG_VERSION")
    );
    let wbody: Vec<u16> = body.encode_utf16().chain(std::iter::once(0)).collect();
    let wtitle: Vec<u16> = "Navigator"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(wbody.as_ptr()),
            PCWSTR(wtitle.as_ptr()),
            MESSAGEBOX_STYLE(MB_OK.0 | MB_ICONINFORMATION.0),
        );
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn dispatch_input(
    ev: nav_input::InputEvent,
    l: &mut LoopCtx,
    input: &nav_input::InputThread,
    uia: &nav_uia::UiaRuntime,
    renderer: &nav_render::Renderer,
    app: &std::sync::Arc<std::sync::Mutex<AppState>>,
    last_focus: &std::sync::Arc<std::sync::Mutex<Option<usize>>>,
    debug_pill_connectors: bool,
) {
    use nav_core::{NavEnumerateResult, SessionEvent, plan};
    use nav_input::SessionKey;
    use std::sync::atomic::Ordering;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    match ev {
        nav_input::InputEvent::Hotkey(p) => {
            println!(
                "[input] hotkey captured_hwnd=0x{:x} latency_us={} plain_slash={}",
                p.captured_hwnd,
                p.latency_us,
                p.from_plain_slash
            );

            if input.hint_mode.load(Ordering::Acquire) {
                l.session = None;
                l.session_hwnd = None;
                l.active_debug_rejects.clear();
                l.overlay_debug_only = false;
                input.hint_mode.store(false, Ordering::Release);
                if let Some(sid) = l.active_show_id.take() {
                    let _ = renderer.hide(sid);
                }
                return;
            }

            if p.captured_hwnd == 0 {
                eprintln!("[uia] skipped: null foreground hwnd snapshot");
                return;
            }

            *last_focus.lock().expect("last_focus") = Some(p.captured_hwnd);

            let hwnd = windows::Win32::Foundation::HWND(p.captured_hwnd as *mut core::ffi::c_void);

            l.overlay_session = l.overlay_session.wrapping_add(1);
            if let Some(prev) = l.active_show_id {
                let _ = renderer.hide(prev);
            }

            let mut freq = 0i64;
            if unsafe { QueryPerformanceFrequency(&mut freq) }.is_err() {
                eprintln!("[uia] QueryPerformanceFrequency failed");
                return;
            }

            let mut t0 = 0i64;
            if unsafe { QueryPerformanceCounter(&mut t0) }.is_err() {
                eprintln!("[pipeline] QueryPerformanceCounter failed");
                return;
            }

            let probe = nav_uia::probe_window(hwnd);
            let cache_key = nav_uia::window_cache_key(hwnd);

            let cache_hit = {
                let st = app.lock().expect("state");
                st.hint_cache.as_ref().is_some_and(|c| {
                    c.hwnd == p.captured_hwnd
                        && c.pid == probe.pid
                        && c.title_fp == cache_key.0
                        && c.rect_ltrb == (cache_key.1, cache_key.2, cache_key.3, cache_key.4)
                        && c.at.elapsed()
                            < std::time::Duration::from_millis(st.hint_cache_ttl_ms.max(1))
                })
            };

            let (raws, debug_rejects) = if cache_hit {
                let (raws_deduped, dbg, age_ms) = {
                    let st = app.lock().expect("state");
                    let c = st.hint_cache.as_ref().expect("cache");
                    (
                        c.raws_deduped.clone(),
                        c.debug_rejects.clone(),
                        c.at.elapsed().as_secs_f64() * 1000.0,
                    )
                };
                eprintln!(
                    "[cache_hit] hwnd=0x{:x} pid={} title_fp=0x{:x} age_ms={:.1}",
                    p.captured_hwnd, probe.pid, cache_key.0, age_ms
                );
                eprintln!("[dedupe] cache_hit=1 dedupe_ms=0.00");
                (raws_deduped, dbg)
            } else {
                let enum_opts = app.lock().expect("state").enum_opts.clone();
                let enum_res = uia.enumerate(hwnd, &enum_opts);

                let mut t1 = 0i64;
                if unsafe { QueryPerformanceCounter(&mut t1) }.is_err() {
                    eprintln!("[enumerate] QueryPerformanceCounter (end) failed");
                    return;
                }

                let enumerate_total_ms = if freq > 0 {
                    (t1.saturating_sub(t0) as f64) * 1000.0 / freq as f64
                } else {
                    0.0
                };

                let NavEnumerateResult {
                    hints: raws_in,
                    debug_rejects,
                    ..
                } = match enum_res {
                    Ok(res) => {
                        print!(
                            "[enumerate] hwnd=0x{:x} elements={} enumerate_total_ms={:.2}",
                            p.captured_hwnd,
                            res.hints.len(),
                            enumerate_total_ms
                        );
                        if let Some(ref t) = res.timings_ms {
                            print!(
                                " findall_ms={:.2} materialize_ms={:.2}",
                                t.findall_ms, t.materialize_ms
                            );
                        }
                        println!();
                        if let Some(ref c) = res.coverage {
                            eprintln!(
                                "[coverage] raw_nodes={} clickable_candidates={} visible={} after_filter={} final={}",
                                c.raw_nodes,
                                c.clickable_candidates,
                                c.visible,
                                c.after_filter,
                                c.final_hints
                            );
                            eprintln!(
                                "[profile_stats] invoke={} toggle={} selection={} expand={} editable={} generic={}",
                                c.kind_invoke,
                                c.kind_toggle,
                                c.kind_select,
                                c.kind_expand,
                                c.kind_editable,
                                c.kind_generic
                            );
                        }
                        res
                    }
                    Err(e) => {
                        eprintln!("[enumerate] error: {e}");
                        return;
                    }
                };

                let mut t_dedupe_0 = 0i64;
                if unsafe { QueryPerformanceCounter(&mut t_dedupe_0) }.is_err() {
                    eprintln!("[dedupe] QueryPerformanceCounter failed");
                    return;
                }

                let (raws, dedupe_stats) = nav_core::dedupe_raw_hints(raws_in);

                let mut t_dedupe_1 = 0i64;
                if unsafe { QueryPerformanceCounter(&mut t_dedupe_1) }.is_err() {
                    eprintln!("[dedupe] QueryPerformanceCounter (end) failed");
                    return;
                }

                let dedupe_ms = if freq > 0 {
                    (t_dedupe_1.saturating_sub(t_dedupe_0) as f64) * 1000.0 / freq as f64
                } else {
                    0.0
                };

                eprintln!(
                    "[dedupe] before={} after={} removed={} dedupe_ms={:.2}",
                    dedupe_stats.before, dedupe_stats.after, dedupe_stats.removed, dedupe_ms
                );

                {
                    let mut st = app.lock().expect("state");
                    st.hint_cache = Some(HintSessionCache {
                        hwnd: p.captured_hwnd,
                        pid: probe.pid,
                        title_fp: cache_key.0,
                        rect_ltrb: (cache_key.1, cache_key.2, cache_key.3, cache_key.4),
                        raws_deduped: raws.clone(),
                        debug_rejects: debug_rejects.clone(),
                        at: std::time::Instant::now(),
                    });
                }

                (raws, debug_rejects)
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

            let (alphabet, planner_cap) = {
                let st = app.lock().expect("state");
                (st.alphabet.clone(), st.planner_label_cap)
            };

            let mut t_plan_0 = 0i64;
            if unsafe { QueryPerformanceCounter(&mut t_plan_0) }.is_err() {
                eprintln!("[plan] QueryPerformanceCounter failed");
                return;
            }

            let n_ranked_in = raws.len();
            let hints = plan(raws, &alphabet, layout_origin, planner_cap);

            if planner_cap > 0 && n_ranked_in > planner_cap {
                eprintln!(
                    "[hint_density] ranked_top={} hidden_count={}",
                    hints.len(),
                    n_ranked_in.saturating_sub(hints.len())
                );
            }

            let mut t_plan_1 = 0i64;
            if unsafe { QueryPerformanceCounter(&mut t_plan_1) }.is_err() {
                eprintln!("[plan] QueryPerformanceCounter (end) failed");
                return;
            }

            let plan_ms = if freq > 0 {
                (t_plan_1.saturating_sub(t_plan_0) as f64) * 1000.0 / freq as f64
            } else {
                0.0
            };

            eprintln!("[plan] plan_ms={:.2}", plan_ms);

            eprintln!("[layout] hints_planned={}", hints.len());

            let pipeline_total_ms = if freq > 0 {
                (t_plan_1.saturating_sub(t0) as f64) * 1000.0 / freq as f64
            } else {
                0.0
            };

            eprintln!("[pipeline] total_ms={:.2}", pipeline_total_ms);

            let (soft, hard) = {
                let st = app.lock().expect("state");
                (st.pipeline_soft_budget_ms, st.pipeline_hard_budget_ms)
            };
            if pipeline_total_ms > hard {
                eprintln!(
                    "[pipeline] HARD budget exceeded total_ms={:.2} hard_ms={:.2} pid={} exe={}",
                    pipeline_total_ms, hard, probe.pid, probe.exe_basename
                );
            } else if pipeline_total_ms > soft {
                eprintln!(
                    "[pipeline] soft budget exceeded total_ms={:.2} soft_ms={:.2} pid={} exe={}",
                    pipeline_total_ms, soft, probe.pid, probe.exe_basename
                );
            }

            let mut sess = nav_core::Session::new(l.overlay_session);
            sess.ingest(hints);
            let initial = sess.visible_hints();

            l.active_debug_rejects = debug_rejects;
            l.overlay_debug_only = initial.is_empty() && !l.active_debug_rejects.is_empty();

            if initial.is_empty() && l.active_debug_rejects.is_empty() {
                l.session = None;
                l.session_hwnd = None;
                l.active_show_id = None;
                l.active_debug_rejects.clear();
                l.overlay_debug_only = false;
                return;
            }

            if let Err(e) = renderer.show(
                l.overlay_session,
                &initial,
                &l.active_debug_rejects,
                nav_render::OverlayRenderOpts {
                    debug_connectors: debug_pill_connectors,
                    ..Default::default()
                },
            ) {
                eprintln!("[render] show: {e}");
                l.session = None;
                l.session_hwnd = None;
                l.active_show_id = None;
                l.active_debug_rejects.clear();
                l.overlay_debug_only = false;
                return;
            }

            l.active_show_id = Some(l.overlay_session);
            l.session = Some(sess);
            l.session_hwnd = Some(hwnd);
            input.hint_mode.store(true, Ordering::Release);
            eprintln!(
                "[render] show_ok session_id={} pills={}",
                l.overlay_session,
                initial.len()
            );
        }
        nav_input::InputEvent::SessionKey(sk) => {
            if !input.hint_mode.load(Ordering::Acquire) {
                return;
            }
            let Some(sid) = l.active_show_id else {
                input.hint_mode.store(false, Ordering::Release);
                return;
            };

            if l.overlay_debug_only {
                if matches!(sk, SessionKey::Escape) {
                    input.hint_mode.store(false, Ordering::Release);
                    let _ = renderer.hide(sid);
                    l.active_show_id = None;
                    l.session = None;
                    l.session_hwnd = None;
                    l.active_debug_rejects.clear();
                    l.overlay_debug_only = false;
                }
                return;
            }

            let Some(mut sess) = l.session.take() else {
                input.hint_mode.store(false, Ordering::Release);
                let _ = renderer.hide(sid);
                l.active_show_id = None;
                l.session_hwnd = None;
                l.active_debug_rejects.clear();
                l.overlay_debug_only = false;
                return;
            };

            let event = match sk {
                SessionKey::Escape => sess.cancel(),
                SessionKey::Backspace => sess.key('\u{8}'),
                SessionKey::Char(c) => sess.key(c),
            };

            match event {
                SessionEvent::Render(_) => {
                    let visible = sess.visible_hints();
                    if let Err(e) = renderer.repaint(
                        sid,
                        &visible,
                        &l.active_debug_rejects,
                        nav_render::OverlayRenderOpts {
                            debug_connectors: debug_pill_connectors,
                            ..Default::default()
                        },
                    ) {
                        eprintln!("[render] repaint: {e}");
                    }
                    l.session = Some(sess);
                }
                SessionEvent::Invoke(id) => {
                    let hwnd = l.session_hwnd.take();
                    let hint = sess.hints().get(id.0 as usize).cloned();
                    input.hint_mode.store(false, Ordering::Release);
                    let _ = renderer.hide(sid);
                    l.active_show_id = None;
                    l.active_debug_rejects.clear();
                    l.overlay_debug_only = false;
                    if let (Some(hwnd), Some(h)) = (hwnd, hint) {
                        let enum_opts = app.lock().expect("state").enum_opts.clone();
                        if let Err(e) = uia.invoke(hwnd, &h, &enum_opts) {
                            eprintln!("[uia] invoke: {e}");
                        }
                    }
                }
                SessionEvent::Done => {
                    input.hint_mode.store(false, Ordering::Release);
                    let _ = renderer.hide(sid);
                    l.active_show_id = None;
                    l.session_hwnd = None;
                    l.active_debug_rejects.clear();
                    l.overlay_debug_only = false;
                }
            }
        }
    }
}
