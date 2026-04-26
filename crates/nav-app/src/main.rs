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
    use clap::Parser;
    use nav_input::{InputEvent, InputThread};
    use nav_uia::{EnumOptions, UiaRuntime};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

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

    let (_input, rx) = match InputThread::spawn() {
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
    let enum_opts = EnumOptions::default();

    println!("Navigator ready");

    while let Ok(InputEvent::Hotkey(p)) = rx.recv() {
        println!(
            "[input] hotkey captured_hwnd=0x{:x} latency_us={}",
            p.captured_hwnd, p.latency_us
        );

        if p.captured_hwnd == 0 {
            eprintln!("[uia] skipped: null foreground hwnd snapshot");
            continue;
        }

        let hwnd = HWND(p.captured_hwnd as *mut core::ffi::c_void);

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

        match enum_res {
            Ok(elements) => {
                println!(
                    "[uia] hwnd=0x{:x} elements={} took_ms={:.2}",
                    p.captured_hwnd,
                    elements.len(),
                    took_ms
                );
            }
            Err(e) => eprintln!("[uia] error: {e}"),
        }
    }

    std::process::ExitCode::SUCCESS
}
