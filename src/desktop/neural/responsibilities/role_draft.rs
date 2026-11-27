//! A role declaration is a transaction. Reviewing a later edition only changes
//! the basis of this window's draft, and only if the operator asks for that.
use super::*;
use ai_personas::neural::task_roles::Role;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::desktop::neural) struct Draft {
    pub(super) work: String,
    pub(super) operator: String,
    brief: String,
    base: Option<String>,
    role: Role,
    note: String,
}

pub(super) struct Terms<'a> {
    pub brief: &'a str,
    pub body: &'a Value,
    pub mine: Option<&'a Value>,
    pub other_owner: bool,
}

impl Terms<'_> {
    pub fn joined(&self) -> bool {
        self.mine.is_some_and(|r| r["state"] == "active")
    }
    fn base(&self) -> Option<&str> {
        self.mine.and_then(|r| r["reference"].as_str())
    }
}

fn edition_link(ui: &mut Ui, label: &str, reference: &str, action: &mut Option<Action>) {
    if ui
        .link(format!("{label} {} ↗", brief_terms::revision(reference)))
        .on_hover_text(reference)
        .clicked()
    {
        *action = Some(Action::Inspect(reference.into()));
    }
}

fn same_or_newer(old: &str, new: &str) -> bool {
    let (Some((a, i)), Some((b, j))) = (old.rsplit_once('@'), new.rsplit_once('@')) else {
        return false;
    };
    a == b
        && i.parse::<u64>()
            .ok()
            .zip(j.parse::<u64>().ok())
            .is_some_and(|(i, j)| i > 0 && j >= i)
}

impl Draft {
    pub(super) fn open(work: &str, operator: &str, now: &Terms) -> Self {
        let active = now.mine.filter(|_| now.joined());
        Self {
            work: work.into(),
            operator: operator.into(),
            brief: now.brief.into(),
            base: now.base().map(str::to_owned),
            role: if active.is_some_and(|r| r["role"] == "owner") {
                Role::Owner
            } else {
                Role::Contributor
            },
            note: active.map_or("", |r| s(r, "note")).into(),
        }
    }

    pub(super) fn command(&self) -> Command {
        Command::TaskRole {
            work: self.work.clone(),
            brief: self.brief.clone(),
            base: self.base.clone(),
            role: self.role,
            note: self.note.trim().into(),
        }
    }

    fn review(&mut self, ui: &mut Ui, now: &Terms, writable: bool, action: &mut Option<Action>) {
        if self.brief != now.brief {
            ui.colored_label(
                theme::ACCENT,
                format!(
                    "Brief {} is current. Your draft still uses brief {}.",
                    brief_terms::revision(now.brief),
                    brief_terms::revision(&self.brief)
                ),
            );
            if let Some(last) = now.body["amendments"].as_array().and_then(|a| a.last()) {
                ui.label(format!(
                    "Latest amendment · {}: {}",
                    s(last, "author"),
                    home::short_text(s(last, "text"), 220)
                ));
            }
            let mut link = None;
            brief_terms::show(ui, now.brief, now.body, "Read current terms", &mut link);
            if let Some(link) = link {
                *action = Some(Action::BriefSource(link));
            }
        }
        if self.base.as_deref() != now.base() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Your recorded part changed.");
                if let Some(base) = now.base() {
                    edition_link(ui, "Current role", base, action);
                } else {
                    ui.weak("No current declaration found.");
                }
            });
        }
        let forward = same_or_newer(&self.brief, now.brief)
            && self
                .base
                .as_deref()
                .is_none_or(|old| now.base().is_some_and(|new| same_or_newer(old, new)));
        if now.body["phase"] != "open" {
            return;
        }
        if !forward {
            ui.weak(
                "The board does not show a successor to this draft’s basis. Refresh before choosing new terms.",
            );
        } else if ui
            .add_enabled(writable, egui::Button::new("Use current terms for this draft"))
            .on_hover_text("Keep the role and note. Nothing is filed until you save.")
            .clicked()
        {
            self.brief = now.brief.into();
            self.base = now.base().map(str::to_owned);
        }
    }

    /// Return true only when the operator explicitly discards the draft.
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        now: &Terms,
        busy: bool,
        saving: bool,
        action: &mut Option<Action>,
    ) -> bool {
        ui.separator();
        ui.strong(format!("My part · {}", self.operator));
        ui.horizontal_wrapped(|ui| {
            ui.small("Draft basis");
            edition_link(ui, "Brief", &self.brief, action);
            if let Some(base) = &self.base {
                edition_link(ui, "Role edition", base, action);
            } else {
                ui.weak("No prior role");
            }
        });
        let current = self.brief == now.brief && self.base.as_deref() == now.base();
        let open = now.body["phase"] == "open";
        if !current {
            self.review(ui, now, !busy, action);
        }
        if !open {
            ui.weak("The activity is not open. You can copy or discard this note, but not file it.");
        } else if self.role == Role::Owner && now.other_owner {
            ui.label("Another person owns the task. You can contribute without taking it over.");
        } else if self.role == Role::Leave && !now.joined() {
            ui.label("You no longer have an active role to leave.");
        }
        ui.add_enabled_ui(open && !busy, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.role, Role::Contributor, "Contribute");
                ui.add_enabled_ui(!now.other_owner, |ui| {
                    ui.selectable_value(&mut self.role, Role::Owner, "Own task")
                });
                if now.joined() || self.role == Role::Leave {
                    ui.selectable_value(&mut self.role, Role::Leave, "Leave role");
                }
            })
        });
        ui.label(match self.role {
            Role::Owner => "What follow-through are you taking on for the task?",
            Role::Contributor => "What are you contributing? You don’t have to own the task.",
            Role::Leave => "What are you leaving unfinished or handing back?",
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.note)
                .id_salt(("role-note", &self.work, &self.operator))
                .desired_width(f32::INFINITY)
                .desired_rows(2),
        );
        if saving {
            ui.small("Saving the submitted wording… Any further edits stay in this window.");
        } else {
            ui.small("Only in this window until saved. Saving declares your part; it does not take an outside step or accept a delivery.");
        }
        ui.horizontal_wrapped(|ui| {
            let valid = open
                && current
                && !busy
                && !self.note.trim().is_empty()
                && (self.role != Role::Owner || !now.other_owner)
                && (self.role != Role::Leave || now.joined());
            if ui
                .add_enabled(
                    valid,
                    theme::primary(if self.role == Role::Leave {
                        "Leave this role"
                    } else {
                        "Save my part"
                    }),
                )
                .clicked()
            {
                *action = Some(Action::SetRole(self.clone()));
            }
            ui.add_enabled(!busy, egui::Button::new("Discard draft").small())
                .clicked()
        })
        .inner
    }
}
