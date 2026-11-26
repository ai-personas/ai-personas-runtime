//! A member can differ at its original path yet survive at another one. Show
//! both observations without turning content identity into a revision history.
use super::*;
use issued_zip::{Locations, Side};

pub(super) enum Action {
    Back,
    Page(usize),
    Open(Side, String),
}

pub(super) fn source(r: &Comparison, side: Side) -> &Source {
    match side {
        Side::Baseline => &r.baseline,
        Side::Selected => &r.selected,
    }
}

pub(super) fn show(app: &App, ui: &mut Ui, r: &Comparison, found: &Locations) -> Option<Action> {
    let mut action = None;
    if ui.link("← Member differences").clicked() {
        action = Some(Action::Back);
    }
    ui.add_space(6.);
    eyebrow(ui, "MEMBER LOOKUP");
    ui.heading("Where these bytes occur");
    let a = source(r, found.source.side);
    let b = source(r, found.searched);
    ui.weak(format!(
        "From {} · {}",
        app.label(&a.at_issue.issuer),
        a.title
    ))
    .on_hover_text(&a.edition);
    ui.add(
        egui::Label::new(RichText::new(&found.source.path).strong())
            .wrap()
            .selectable(true),
    );
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, &format!("{} bytes", found.fingerprint.bytes), TEAL);
        ui.small(format!("SHA-256 {}…", &found.fingerprint.sha256[..12]))
            .on_hover_text(&found.fingerprint.sha256);
        if ui.link("Inspect source ↗").clicked() {
            action = Some(Action::Open(found.source.side, found.source.path.clone()));
        }
    });
    ui.add_space(6.);
    ui.separator();
    heading(ui, "In the other ZIP");
    ui.add(egui::Label::new(&b.title).wrap())
        .on_hover_text(&b.edition);
    ui.add(egui::Label::new(RichText::new(&b.path).small()).wrap());
    let (label, tint) = match &found.at_original_path {
        None => ("Original path absent", PURPLE),
        Some(fp) if fp == &found.fingerprint => ("Same bytes at original path", TEAL),
        Some(_) => ("Different bytes at original path", theme::ACCENT),
    };
    ui.horizontal_wrapped(|ui| {
        theme::badge(ui, label, tint);
        if let Some(fp) = found
            .at_original_path
            .as_ref()
            .filter(|fp| *fp != &found.fingerprint)
        {
            if ui
                .link(format!("Inspect {} bytes there ↗", fp.bytes))
                .on_hover_text(&found.source.path)
                .clicked()
            {
                action = Some(Action::Open(found.searched, found.source.path.clone()));
            }
        }
    });
    ui.add_space(6.);
    ui.strong(format!(
        "{} · {} checked",
        counted(found.matching as u64, "matching path", "matching paths"),
        counted(found.searched_files as u64, "file", "files")
    ));
    if found.matching == 0 {
        ui.weak("No regular file in this archive has the source member’s length and SHA-256.");
    }
    for path in &found.paths {
        ui.horizontal_wrapped(|ui| {
            theme::glyph(ui, Icon::Files, TEAL);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            if ui
                .link(path)
                .on_hover_text("Inspect this member from the other issued ZIP")
                .clicked()
            {
                action = Some(Action::Open(found.searched, path.clone()));
            }
        });
    }
    let mut page = found.offset / PAGE;
    home::pager(ui, &mut page, found.matching, PAGE, "matching paths");
    if page != found.offset / PAGE {
        action = Some(Action::Page(page * PAGE));
    }
    ui.add_space(8.);
    ui.weak("Original paths and uncompressed contents. A match does not show who copied, renamed or used a file.");
    ui.collapsing("Lookup details", |ui| {
        for (label, s) in [("Source", a), ("Searched", b)] {
            ui.strong(label);
            ui.add(
                egui::Label::new(format!(
                    "{}\n{}\n{}\nZIP SHA-256 {}",
                    s.edition, s.file, s.path, s.sha256
                ))
                .wrap()
                .selectable(true),
            );
        }
        ui.add(
            egui::Label::new(format!(
                "Source member: {} bytes\nSHA-256 {}",
                found.fingerprint.bytes, found.fingerprint.sha256
            ))
            .wrap()
            .selectable(true),
        );
        if let Some(other) = &found.at_original_path {
            ui.add(
                egui::Label::new(format!(
                    "Other ZIP at original path: {} bytes\nSHA-256 {}",
                    other.bytes, other.sha256
                ))
                .wrap()
                .selectable(true),
            );
        }
    });
    action
}
