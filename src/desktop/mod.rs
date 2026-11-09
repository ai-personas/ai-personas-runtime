mod artifacts;
mod document;
mod data_view;
mod backend;
mod providers;
mod helpers;
mod journal;
mod mesh;
mod theme;
mod timings;
mod ui;
mod neural;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "personas-desktop",
    version,
    about = "Personas desktop workspace for Linux and macOS"
)]
struct Options {
    /// Use this saved-progress folder instead of the discovered default.
    #[arg(long)]
    progress: Option<PathBuf>,
    /// Directory containing the four matching native runtime helpers.
    #[arg(long)]
    helpers: Option<PathBuf>,
}
pub fn run() -> eframe::Result {
    let options = Options::parse();
    let native = eframe::NativeOptions {
        depth_buffer: 24,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Personas")
            .with_inner_size([1180.0, 800.0])
            .with_min_inner_size([780.0, 560.0])
            .with_app_id("org.ai-personas.desktop"),
        ..Default::default()
    };
    eframe::run_native(
        "Personas",
        native,
        Box::new(move |cc| {
            if let Some(root)=options.progress.as_ref().filter(|root|root.join("neural.sqlite").is_file()) {
                return Ok(Box::new(neural::App::new(cc,root.clone())));
            }
            Ok(Box::new(ui::App::new(
                cc,
                options.progress,
                options.helpers,
            )))
        }),
    )
}
