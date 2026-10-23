//! Optional host timings. These never enter persona context or durable state.
use std::io::Write;
use std::sync::{atomic::{AtomicU64, Ordering}, OnceLock};
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(super) fn measure<T, E>(
    phase: &'static str,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    if !*ENABLED.get_or_init(|| std::env::var_os("AI_PERSONAS_RUNTIME_TIMINGS").as_deref()
        == Some(std::ffi::OsStr::new("1")))
    {
        return operation();
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let result = operation();
    let event = serde_json::json!({
        "schema": "runtime-phase-timing-v1",
        "pid": std::process::id(),
        "sequence": sequence,
        "phase": phase,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "ok": result.is_ok(),
    });
    // Diagnostic output failure must not replace the operation's result.
    let _ = writeln!(std::io::stderr().lock(), "{event}");
    result
}
