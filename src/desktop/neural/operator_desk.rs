//! One named operator per workspace window. Open another window rather than
//! relabeling pending messages, field observations, findings or file pickers.
use super::{s, Value};
use eframe::egui;
use std::{io, path::Path};

pub(super) fn open_window(root: Option<&Path>, operator: &str) -> io::Result<()> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.arg(format!("--operator={operator}"));
    if let Some(root) = root {
        command.arg("--progress").arg(root);
    }
    // The child opens Studio normally. No run, invitation, restore or draft
    // transfer. Reap it if it closes while the original desk remains open.
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// Independent of the task/participation dialog: asking a colleague to open
// their desk must not close the current operator's half-written invitation.
#[derive(Default)]
pub(super) struct Launcher {
    name: Option<String>,
    focus: bool,
    error: Option<String>,
}

impl Launcher {
    pub fn open(&mut self) {
        self.name.get_or_insert_with(String::new);
        self.focus = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, root: &Path, operator: &str) -> Option<String> {
        let name = self.name.as_mut()?;
        let bounds = ctx.content_rect().shrink(6.);
        let chrome = super::theme::titled_window_chrome(ctx);
        let room = (bounds.size() - chrome).max(egui::vec2(1., 1.));
        let size = egui::vec2(390., 218.).min(room);
        let mut visible = true;
        let mut cancelled = false;
        let mut started = None;
        egui::Window::new("Operator desk").open(&mut visible).constrain_to(bounds)
            .min_size(egui::vec2(140., 68.).min(room)).max_size(room).default_size(size)
            .default_pos(bounds.center() - (size + chrome) * 0.5).vscroll(true)
            .show(ctx, |ui| {
                ui.strong(format!("Current desk · {operator}"));
                ui.label("Open this workspace in another window with its own operator name. Your drafts and any active run stay here.");
                ui.small("Both windows can read the workspace. A desk name attributes actions; it is not a sign-in or a task role.");
                ui.add_space(6.);
                ui.label("Name for the other desk");
                let field = ui.add(egui::TextEdit::singleline(name).hint_text("Operator name").desired_width(f32::INFINITY));
                if std::mem::take(&mut self.focus) {
                    ctx.move_to_top(ui.layer_id());
                    field.request_focus();
                }
                let next = name.trim();
                let ready = !next.is_empty() && next != operator;
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let clicked = ui.horizontal_wrapped(|ui| {
                    let clicked = ui.add_enabled(ready, super::theme::primary("Open desk")).clicked();
                    cancelled = ui.button("Cancel").clicked();
                    clicked
                }).inner;
                if ready && (clicked || enter) {
                    match open_window(Some(root), next) {
                        Ok(()) => started = Some(next.to_owned()),
                        Err(error) => self.error = Some(format!("Could not start Studio: {error}")),
                    }
                }
                if let Some(error) = &self.error { ui.colored_label(ui.visuals().error_fg_color, error); }
            });
        if !visible || cancelled || started.is_some() {
            *self = Self::default();
        }
        started
    }
}

/// A journal address such as `user` (the shared recipient) is not a named
/// writer. For operator-originated entries use the recorded actor, not a
/// matching persona label. Only typed operator address fields have a prefix.
pub(super) fn author(event: &Value) -> Option<&str> {
    if !event["invocation"].is_null() {
        return None;
    }
    let actor = event["actor"].as_str()?;
    match s(event, "kind") {
        "invitation"
        | "user_message"
        | "human_request_progress"
        | "user_acceptance"
        | "requirements_amended"
        | "explicit_resume"
        | "activity_dissolved"
        | "activity_reopened"
        | "operator_render" => Some(actor),
        "human_role_changed" | "asset_feedback" | "package_review" | "agreement_proposed"
        | "agreement_response" => actor.strip_prefix("operator:"),
        _ => None,
    }
}
