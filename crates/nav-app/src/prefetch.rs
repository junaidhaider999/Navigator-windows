//! Background foreground-window prefetcher.
//!
//! Polls [`GetForegroundWindow`] at ~6 Hz from a dedicated STA thread that owns its own
//! [`nav_uia::UiaRuntime`]. When the foreground HWND changes (or the cache for the same HWND
//! has aged past a short staleness window), the prefetcher enumerates that window in the
//! background and ships a ready-to-use [`CachedEnumeration`] across a channel to the main loop.
//!
//! Main loop folds incoming entries into `AppState.hint_cache`. On the next hotkey press the
//! synchronous dispatch path sees a `cache_hit` and skips the ~10–25 ms `FindAllBuildCache` —
//! making repeat activations on the same window feel instant.
//!
//! Design notes:
//! * Polling instead of `SetWinEventHook`: avoids cross-thread COM marshaling and a message
//!   pump in this worker; foreground transitions are perceptually slow (alt-tab) so 150 ms
//!   sampling is sufficient and stays trivially cheap (`GetForegroundWindow` is ~50 ns).
//! * The worker keeps its own UIA singleton (STA-bound): UI Automation objects are not `Send`
//!   so the runtime is constructed *inside* the spawned thread.
//! * `Arc<Mutex<EnumOptions>>` lets config-reload updates propagate without re-spawning.
//! * Window-key parity with [`crate::HintSessionCache`] is enforced by a shared struct so the
//!   main loop can move entries into the live cache without re-validation.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use nav_core::{RawHint, UiaDebugReject};
use nav_uia::{EnumOptions, UiaRuntime};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// One enumerated foreground window ready for the main hint cache.
///
/// Field set mirrors `HintSessionCache` in `main.rs` so the main loop can store it verbatim.
#[derive(Clone)]
pub(crate) struct CachedEnumeration {
    pub hwnd: usize,
    pub pid: u32,
    pub title_fp: u64,
    pub rect_ltrb: (i32, i32, i32, i32),
    pub raws_deduped: Vec<RawHint>,
    pub debug_rejects: Vec<UiaDebugReject>,
    pub at: Instant,
}

pub(crate) struct Prefetcher {
    pub rx: Receiver<CachedEnumeration>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Prefetcher {
    pub fn spawn(opts: Arc<Mutex<EnumOptions>>) -> std::io::Result<Self> {
        let (tx, rx) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = stop.clone();
        let thread = std::thread::Builder::new()
            .name("navigator-prefetch".into())
            .spawn(move || run_loop(stop_w, opts, tx))?;
        Ok(Self {
            rx,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Polls the foreground HWND and emits `CachedEnumeration` entries when:
/// * the HWND changed since the last enumeration, **or**
/// * the same HWND has been the foreground for `STALE_REFRESH_MS` since the last enumeration
///   (covers in-place content changes — open a menu, navigate a page, etc.).
fn run_loop(
    stop: Arc<AtomicBool>,
    opts: Arc<Mutex<EnumOptions>>,
    tx: Sender<CachedEnumeration>,
) {
    // Stagger so the very first poll doesn't race the main-thread `UiaRuntime::new()`
    // (both create a UIA singleton — separate STAs, but COM init contention is real).
    std::thread::sleep(Duration::from_millis(400));

    let uia = match UiaRuntime::new() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[prefetch] uia init failed: {e}; foreground prefetch disabled");
            return;
        }
    };

    /// Foreground sample interval. Below ~50 ms it would steal CPU; above 250 ms it would
    /// noticeably miss fast alt-tab flicks. 150 ms is the perceptual sweet spot.
    const POLL_MS: u64 = 150;
    /// Re-enumerate the same foreground HWND when its previous cache entry has aged past this
    /// limit (kept slightly below `HintsConfig::hint_cache_ttl_ms` so the dispatch path still
    /// sees a fresh hit when the user dwells).
    const STALE_REFRESH_MS: u128 = 800;

    let mut last_bits: usize = 0;
    let mut last_enum_at = Instant::now() - Duration::from_secs(60);

    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            continue;
        }
        let bits = hwnd.0 as usize;
        if bits == 0 {
            continue;
        }

        let probe = nav_uia::probe_window(hwnd);
        // Don't enumerate ourselves — Navigator's overlay HWND is not a useful prefetch target.
        if probe
            .exe_basename
            .eq_ignore_ascii_case(env!("CARGO_PKG_NAME"))
            || probe.exe_basename.eq_ignore_ascii_case("navigator.exe")
        {
            continue;
        }

        let stale = last_enum_at.elapsed().as_millis() >= STALE_REFRESH_MS;
        if bits == last_bits && !stale {
            continue;
        }

        // Snapshot opts under lock for the duration of this enumeration only.
        let opts_snap = match opts.lock() {
            Ok(g) => g.clone(),
            Err(_) => continue,
        };
        let cache_key = nav_uia::window_cache_key(hwnd);

        let res = match uia.enumerate(hwnd, &opts_snap) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if res.hints.is_empty() {
            // Empty enumeration is not worth caching — let the synchronous path try
            // fallback ladders (MSAA / raw HWND).
            last_bits = bits;
            last_enum_at = Instant::now();
            continue;
        }

        let (raws, _stats) = nav_core::dedupe_raw_hints(res.hints);

        last_bits = bits;
        last_enum_at = Instant::now();

        let entry = CachedEnumeration {
            hwnd: bits,
            pid: probe.pid,
            title_fp: cache_key.0,
            rect_ltrb: (cache_key.1, cache_key.2, cache_key.3, cache_key.4),
            raws_deduped: raws,
            debug_rejects: res.debug_rejects,
            at: Instant::now(),
        };

        if tx.send(entry).is_err() {
            // Receiver dropped — main loop is shutting down.
            break;
        }
    }
}
