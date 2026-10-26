//! Explicit foreground interruption, shared by transport and host workers.
//!
//! CLI entry points install this once inside their Tokio runtime and retain the
//! guard until their owned work has drained. Library callers opt in explicitly.
//! The signal remains set for this process; it never resumes work or proves that
//! a remote operation stopped.
use std::sync::atomic::{AtomicU8, Ordering};

static SIGNAL: AtomicU8 = AtomicU8::new(0);

pub struct Guard(tokio::task::JoinHandle<()>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Register before starting foreground work. Actual cancellation and durable
/// settlement happen in the owning coordinator, never in a signal handler.
pub fn install() -> std::io::Result<Guard> {
    #[cfg(unix)]
    let task = {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::spawn(async move {
            let received = tokio::select! {
                _ = interrupt.recv() => 1,
                _ = terminate.recv() => 2,
            };
            SIGNAL.store(received, Ordering::Relaxed);
        })
    };
    #[cfg(not(unix))]
    let task = tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            SIGNAL.store(1, Ordering::Relaxed);
        }
    });
    Ok(Guard(task))
}

pub fn signal() -> Option<&'static str> {
    match SIGNAL.load(Ordering::Relaxed) {
        1 => Some("SIGINT"),
        2 => Some("SIGTERM"),
        _ => None,
    }
}
