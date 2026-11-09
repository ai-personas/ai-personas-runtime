//! Opt-in host diagnostics; no task contents or provider data are logged.
use eframe::egui;
use std::{io::Write, sync::OnceLock, time::Instant};

static ENABLED: OnceLock<bool> = OnceLock::new();

pub struct Frame {
    started: Instant,
    context: egui::Context,
    previous_cpu_seconds: Option<f32>,
}

pub fn frame(context: &egui::Context, frame: &eframe::Frame) -> Option<Frame> {
    (*ENABLED.get_or_init(|| std::env::var_os("AI_PERSONAS_DESKTOP_TIMINGS").as_deref()
        == Some(std::ffi::OsStr::new("1"))))
        .then(|| Frame {
            started: Instant::now(), context: context.clone(),
            previous_cpu_seconds: frame.info().cpu_usage,
        })
}

impl Drop for Frame {
    fn drop(&mut self) {
        let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let causes: Vec<_> = self.context.repaint_causes().iter().map(|c| format!("{c:?}")).collect();
        let event = serde_json::json!({
            "schema":"desktop-frame-timing-v1", "pid":std::process::id(),
            "ui_ms":elapsed_ms, "previous_frame_cpu_seconds":self.previous_cpu_seconds,
            "repaint_causes":causes,
            "input_event_kinds": self.context.input(|input| input.events.iter().map(|event| match event {
                egui::Event::PointerMoved(_) => "pointer_moved",
                egui::Event::MouseMoved(_) => "mouse_moved",
                egui::Event::PointerButton { .. } => "pointer_button",
                egui::Event::MouseWheel { .. } => "wheel",
                egui::Event::Key { .. } => "key",
                egui::Event::Text(_) => "text",
                egui::Event::WindowFocused(_) => "focus",
                _ => "other",
            }).collect::<Vec<_>>()),
        });
        let _ = writeln!(std::io::stderr().lock(), "{event}");
    }
}
