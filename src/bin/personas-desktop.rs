//! Native foreground UI. The existing Rust helpers own all runtime operations.
#[cfg(unix)]
#[path = "../desktop/mod.rs"]
mod desktop;

#[cfg(unix)]
fn main() -> eframe::Result {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("wasi-worker")) {
        if let Err(error) = ai_personas::neural::commands::wasi_worker() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("compute-worker")) {
        if let Err(error)=ai_personas::neural::workspace::compute_worker() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    desktop::run()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("Personas Desktop currently supports Linux and macOS.");
    std::process::exit(1);
}
