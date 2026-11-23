use super::*;

#[derive(Default)]
pub(super) struct Form {
    archive: String,
    destination: String,
    select_project: bool,
    project: String,
}

fn location(ui: &mut egui::Ui, title: &str, path: &mut String, folder: bool) -> bool {
    let label = ui.strong(title);
    let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let mut picked = false;
        if ui.button("Browse…").clicked() {
            let picker = rfd::FileDialog::new().set_title(title);
            if let Some(p) = if folder { picker.pick_folder() } else { picker.pick_file() } {
                *path = p.display().to_string();
                picked = true;
            }
        }
        let size = egui::vec2(ui.available_width().max(1.), ui.spacing().interact_size.y);
        ui.add_sized(size, egui::TextEdit::singleline(path)
            .hint_text(if folder { "Folder path" } else { "Archive path" }))
            .labelled_by(label.id).changed() || picked
    }).inner
}

impl App {
    pub(super) fn show_restore_desk(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.restore_desk.take() else { return };
        let mut open = true;
        let mut submit = false;
        let width = (ctx.content_rect().width() - 60.).clamp(1., 520.);
        egui::Window::new("Restore workspace").open(&mut open).collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).resizable(false)
            .min_width(1.).max_width(width).default_width(width)
            .max_height((ctx.content_rect().height()-90.).max(70.)).vscroll(true)
            .show(ctx, |ui| {
                ui.set_width((width - ui.spacing().scroll.allocated_width()).max(1.));
                ui.label(design::display("Restore a society", 28.));
                ui.label("Bring its people, decisions and issued work into a new workspace.");
                ui.add_space(8.);
                let mut edited = location(ui, "Workspace archive", &mut form.archive, false);
                edited |= location(ui, "Destination parent folder", &mut form.destination, true);
                ui.add_space(6.);
                ui.strong("Project files for future work");
                edited |= ui.radio_value(&mut form.select_project, false, "Use the folder saved in the archive").changed();
                edited |= ui.radio_value(&mut form.select_project, true, "Use a different project folder").changed();
                if form.select_project {
                    edited |= location(ui, "Project folder for this copy", &mut form.project, true);
                    ui.small("Choose an existing folder. External project files are not copied during restore.");
                }
                if edited { self.error = None; }
                ui.separator();
                let archive = journal::expand(&PathBuf::from(form.archive.trim()));
                if let Some(stem) = archive.file_stem().filter(|_| !form.destination.trim().is_empty()) {
                    let root = journal::expand(&PathBuf::from(form.destination.trim()))
                        .join(format!("{}-restored", stem.to_string_lossy()));
                    ui.add(egui::Label::new(format!("Restore to {}", root.display())).wrap());
                }
                if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, error); }
                let ready = !form.archive.trim().is_empty() && !form.destination.trim().is_empty()
                    && (!form.select_project || !form.project.trim().is_empty());
                ui.horizontal(|ui| {
                    submit = ui.add_enabled(!self.busy && ready, theme::primary("Restore and open")).clicked();
                    if self.busy { ui.spinner(); }
                });
            });
        if submit {
            let archive = journal::expand(&PathBuf::from(form.archive.trim()));
            let name = archive.file_stem().and_then(|s| s.to_str()).unwrap_or("workspace");
            let root = journal::expand(&PathBuf::from(form.destination.trim())).join(format!("{name}-restored"));
            let project = form.select_project.then(|| journal::expand(&PathBuf::from(form.project.trim())));
            self.restore_desk = Some(form);
            self.restore(ctx, archive, root, project);
        } else if open {
            self.restore_desk = Some(form);
        }
    }
}
