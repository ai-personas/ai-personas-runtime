//! Native foreground UI. The existing Rust helpers own all runtime operations.
#[cfg(unix)]
#[path = "../desktop/mod.rs"]
mod desktop;

#[cfg(unix)]
fn main() -> eframe::Result {
    desktop::run()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("Personas Desktop currently supports Linux and macOS.");
    std::process::exit(1);
}
