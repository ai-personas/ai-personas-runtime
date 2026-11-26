mod artifacts;
mod folder_nav;
mod file_desk;
mod page_view;
mod document;
mod html_view;
mod data_view;
mod backend;
mod providers;
mod helpers;
mod journal;
mod mesh;
mod theme;
mod design;
mod timings;
mod ui;
mod neural;
mod start;

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
    /// Local author name for numerical workspace actions in this window.
    #[arg(long, default_value = "user")]
    operator: String,
    /// Open a local file in the Studio file viewer. May be repeated.
    #[arg(long = "open-file", value_name = "FILE")]
    files: Vec<PathBuf>,
    /// Files supplied by a file manager or as positional arguments.
    #[arg(value_name = "FILES")]
    paths: Vec<PathBuf>,
    /// Directory containing the four matching native runtime helpers.
    #[arg(long)]
    helpers: Option<PathBuf>,
}
pub fn run() -> eframe::Result {
    let mut options = Options::parse();
    options.files.append(&mut options.paths);
    let native = eframe::NativeOptions {
        depth_buffer: 24,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Persona Studio")
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("art/studio-mark.png")).expect("embedded Studio icon"))
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([780.0, 560.0])
            .with_app_id("org.ai-personas.desktop"),
        ..Default::default()
    };
    eframe::run_native(
        "Personas",
        native,
        Box::new(move |cc| {
            // An explicitly opened historical desktop journal retains its own
            // viewer. New workspaces use the editable-model runtime directly.
            let app: Box<dyn eframe::App> = if options.progress.as_ref().is_some_and(|root| !root.join("neural.sqlite").is_file() && root.join("progress.json").is_file()) {
                Box::new(ui::App::new(cc, options.progress, options.helpers))
            } else {
                Box::new(start::App::new(cc, options.progress, options.operator))
            };
            Ok(Box::new(file_desk::Host::new(app, cc, options.files)))
        }),
    )
}
