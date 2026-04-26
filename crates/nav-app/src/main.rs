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

    println!("Navigator ready");

    while let Ok(InputEvent::Hotkey(p)) = rx.recv() {
        println!(
            "[input] hotkey captured_hwnd=0x{:x} latency_us={}",
            p.captured_hwnd, p.latency_us
        );
    }

    std::process::ExitCode::SUCCESS
}
