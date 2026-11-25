//! An operator’s verdict belongs to an issued edition and the terms they chose,
//! not to whatever brief happens to be current when the writer gets the lock.
use super::*;

use brief_terms::revision;

/// Older records tell us when a review was saved, not which terms its author selected.
pub(super) fn summary(ui: &mut Ui, review: &Value) -> bool {
    let Some(accepted) = review["accepted"].as_bool() else {
        return false;
    };
    let latest = review["decision_count"]
        .as_u64()
        .filter(|n| *n > 1)
        .map(|n| format!("Latest of {n} decisions · "))
        .unwrap_or_default();
    let verdict = if accepted {
        "Accepted"
    } else {
        "Changes requested"
    };
    let basis = review["reviewed_brief"]
        .as_str()
        .map(|r| format!("brief {}", revision(r)))
        .unwrap_or_else(|| "review basis not recorded".into());
    ui.small(format!(
        "{latest}{verdict} · {} · {basis}",
        s(review, "reviewer")
    ))
    .on_hover_text(s(review, "reference"));
    true
}

fn basis<'a>(app: &'a App, edition: &str) -> Option<(&'a str, &'a str, &'a Value)> {
    let mandates = &app.snapshot.status["delivery_mandates"][edition];
    let current = mandates["current_brief"].as_str()?;
    let work = app
        .records("work")
        .iter()
        .find(|r| inbox::handle(r) == current)?;
    Some((
        mandates["delivered_under"].as_str()?,
        current,
        &work["body"],
    ))
}

pub(super) struct Form {
    pub edition: String,
    title: String,
    issued_under: String,
    reviewed: String,
    terms: Value,
    confirmed_head: String,
    reason: String,
    pub retry: Option<(String, String)>,
}

impl Form {
    pub(super) fn open(app: &App, edition: &str, title: &str) -> Option<Self> {
        let (issued, current, terms) = basis(app, edition)?;
        Some(Self {
            edition: edition.into(),
            title: title.into(),
            issued_under: issued.into(),
            reviewed: current.into(),
            terms: terms.clone(),
            confirmed_head: current.into(),
            reason: String::new(),
            retry: None,
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, app: &mut App) -> Option<Command> {
        eyebrow(ui, "Delivery  /  Operator decision");
        ui.heading(&self.title).on_hover_text(&self.edition);
        ui.small(format!(
            "Issued under brief {} · Reviewing as {}",
            revision(&self.issued_under),
            app.operator
        ))
        .on_hover_text(&self.issued_under);
        ui.separator();
        let mut inspect = None;
        brief_terms::show(
            ui,
            &self.reviewed,
            &self.terms,
            "Review basis",
            &mut inspect,
        );
        let mut ready = false;
        let mut load_issued = false;
        let mut refreshed = false;
        if let Some((issued, current, now)) =
            basis(app, &self.edition).filter(|(i, _, _)| *i == self.issued_under)
        {
            let stale = current != self.confirmed_head;
            if stale {
                ui.colored_label(
                    theme::ACCENT,
                    "The brief changed. Your draft has not moved.",
                );
                if let Some(amendment) = now["amendments"].as_array().and_then(|a| a.last()) {
                    ui.strong(format!("Latest amendment · {}", s(amendment, "author")));
                    ui.label(document::excerpt(s(amendment, "text"), 240));
                }
                brief_terms::show(ui, current, now, "Current terms", &mut inspect);
            }
            if self.reviewed != current {
                if !stale {
                    ui.small(format!(
                        "Current brief: {}. This decision applies only to brief {}.",
                        revision(current),
                        revision(&self.reviewed)
                    ));
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(format!("Use brief {}", revision(current)))
                        .clicked()
                    {
                        self.reviewed = current.into();
                        self.terms = now.clone();
                        self.confirmed_head = current.into();
                        refreshed = true;
                    }
                    if stale
                        && ui
                            .button(format!("Keep brief {} only", revision(&self.reviewed)))
                            .clicked()
                    {
                        self.confirmed_head = current.into();
                        refreshed = true;
                    }
                });
            } else if !stale && self.reviewed != issued {
                load_issued = ui
                    .link(format!("Use brief {} as issued", revision(issued)))
                    .clicked();
            }
            ready = !stale;
        } else {
            ui.colored_label(
                theme::ACCENT,
                "The issuing or current brief is unavailable. Your draft is kept.",
            );
        }
        if refreshed {
            app.error = None;
        }
        if load_issued {
            match data::open(&app.root)
                .and_then(|db| ai_personas::neural::store::exact(&db, &self.issued_under))
            {
                Ok(record)
                    if record.reference.handle() == self.issued_under
                        && record.reference.kind == "work"
                        && record.body["id"] == self.terms["id"] =>
                {
                    self.reviewed.clone_from(&self.issued_under);
                    self.terms = record.body;
                    app.error = None;
                }
                Ok(_) => {
                    app.error = Some("The issue brief is unavailable for this activity.".into())
                }
                Err(e) => app.error = Some(e.to_string()),
            }
        }
        if let Some(link) = inspect {
            link.open(app, ui.ctx());
        }
        ui.add_space(5.);
        ui.label("Decision note");
        ui.add(
            egui::TextEdit::multiline(&mut self.reason)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text("What meets these terms or needs to change?"),
        );
        ui.small("Records your decision on this edition. Does not resume work.");
        let mut command = None;
        ui.horizontal_wrapped(|ui| {
            for (reject, label) in [(false, "Accept edition"), (true, "Request changes")] {
                if ui
                    .add_enabled(
                        ready && !self.reason.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    command = Some(Command::Accept {
                        delivery: self.edition.clone(),
                        brief: self.reviewed.clone(),
                        current_brief: (self.confirmed_head != self.reviewed)
                            .then(|| self.confirmed_head.clone()),
                        reject,
                        reason: self.reason.clone(),
                    });
                }
            }
        });
        command
    }
}
