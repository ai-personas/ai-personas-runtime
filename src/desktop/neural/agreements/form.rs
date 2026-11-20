use super::*;
use model::{Position, Proposal, Response};

enum Draft {
    Terms(Proposal),
    Position {
        edition: String,
        previous: Option<String>,
        choice: Option<Position>,
        reason: String,
        scope: String,
        terms: String,
        can_withdraw: bool,
        withdrawal_only: bool,
    },
}

pub(in crate::desktop::neural) struct Form {
    work: String,
    operator: String,
    subject: String,
    draft: Draft,
    request: Option<String>,
}

impl Form {
    pub(super) fn proposal(
        work: &str,
        operator: &str,
        report: &Value,
        prior: Option<&Value>,
    ) -> Self {
        let record = prior.unwrap_or(&Value::Null);
        Self {
            work: work.into(),
            operator: operator.into(),
            subject: String::new(),
            request: None,
            draft: Draft::Terms(Proposal {
                brief: s(report, "brief").into(),
                base: prior.map(|_| s(report, "reference").into()),
                title: s(record, "title").into(),
                scope: s(record, "scope").into(),
                terms: s(record, "terms").into(),
            }),
        }
    }
    pub(super) fn response(
        work: &str,
        operator: &str,
        detail: &Value,
        withdrawal_only: bool,
    ) -> Self {
        let prior = &detail["your_position"];
        let standing = prior["body"]["position"]
            .as_str()
            .is_some_and(|s| s != "withdraw");
        Self {
            work: work.into(),
            operator: operator.into(),
            subject: s(&detail["record"], "title").into(),
            request: None,
            draft: Draft::Position {
                edition: s(detail, "reference").into(),
                previous: prior.is_object().then(|| inbox::handle(prior)),
                choice: withdrawal_only.then_some(Position::Withdraw),
                reason: String::new(),
                scope: s(&detail["record"], "scope").into(),
                terms: s(&detail["record"], "terms").into(),
                can_withdraw: standing,
                withdrawal_only,
            },
        }
    }
    pub(in crate::desktop::neural) fn title(&self) -> &'static str {
        match &self.draft {
            Draft::Terms(p) if p.base.is_some() => "Revise agreement",
            Draft::Terms(_) => "Propose terms",
            Draft::Position { .. } => "Record my position",
        }
    }
    pub(in crate::desktop::neural) fn request_id(&mut self) -> ai_personas::neural::Result<String> {
        if let Some(key) = &self.request {
            return Ok(key.clone());
        }
        let key = ai_personas::neural::id("desk-agreement")?;
        self.request = Some(key.clone());
        Ok(key)
    }
    pub(in crate::desktop::neural) fn show(&mut self, ui: &mut Ui, app: &App) -> Option<Command> {
        let activity = app
            .records("work")
            .iter()
            .find(|r| r["reference"]["id"] == self.work)?;
        let current = inbox::handle(activity);
        let open = activity["body"]["phase"] != "dissolved" && self.operator == app.operator.trim();
        let title = activity_title(&activity["body"]);
        ui.add(egui::Label::new(RichText::new(title).small().weak()).truncate())
            .on_hover_text(title);
        ui.small(format!("Recording as {} · Operator", self.operator));
        if !open {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "This draft requires its original operator and an open activity.",
            );
        }
        ui.separator();
        match &mut self.draft {
            Draft::Terms(p) => {
                if let Some(prior) = &p.base {
                    ui.weak(format!("Revises edition {}", version(prior)));
                }
                ui.label("Title");
                single(ui, &mut p.title);
                ui.label("Scope");
                multiline(ui, &mut p.scope, 2);
                ui.label("Terms");
                multiline(ui, &mut p.terms, 4);
                if p.brief != current {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "The activity brief has changed.",
                    );
                    ui.label(s(&activity["body"], "original_need"));
                    for amendment in activity["body"]["amendments"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        ui.label(s(amendment, "text"));
                    }
                    if ui.button("Use this brief for my draft").clicked() {
                        p.brief = current.clone();
                    }
                }
                let ready = open
                    && p.brief == current
                    && [&p.title, &p.scope, &p.terms]
                        .iter()
                        .all(|s| !s.trim().is_empty());
                ui.add_space(5.);
                if ui
                    .add_enabled(ready, theme::primary("Publish terms"))
                    .clicked()
                {
                    return Some(Command::ProposeAgreement {
                        work: self.work.clone(),
                        proposal: p.clone(),
                    });
                }
            }
            Draft::Position {
                edition,
                previous,
                choice,
                reason,
                scope,
                terms,
                can_withdraw,
                withdrawal_only,
            } => {
                ui.strong(&self.subject);
                ui.small(format!("Edition {}", version(edition)));
                ui.collapsing("Read the terms", |ui| {
                    ui.weak(scope.as_str());
                    ui.label(terms.as_str());
                });
                ui.horizontal_wrapped(|ui| {
                    if !*withdrawal_only {
                        ui.selectable_value(choice, Some(Position::Endorse), "Endorse");
                        ui.selectable_value(choice, Some(Position::Dissent), "Dissent");
                    }
                    if *can_withdraw {
                        ui.selectable_value(choice, Some(Position::Withdraw), "Withdraw");
                    }
                });
                ui.label("Reason");
                multiline(ui, reason, 3);
                ui.add_space(5.);
                if ui
                    .add_enabled(
                        open && choice.is_some() && !reason.trim().is_empty(),
                        theme::primary("Sign this position"),
                    )
                    .clicked()
                {
                    return Some(Command::AgreementPosition {
                        work: self.work.clone(),
                        response: Response {
                            agreement: edition.clone(),
                            previous: previous.clone(),
                            position: (*choice)?,
                            reason: reason.clone(),
                        },
                    });
                }
            }
        }
        None
    }
}

fn single(ui: &mut Ui, text: &mut String) {
    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
}
fn multiline(ui: &mut Ui, text: &mut String, rows: usize) {
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
}
