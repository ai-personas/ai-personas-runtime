use super::*;
use ai_personas::neural::live::{RunScope, RunnerObservation};
use std::sync::atomic::Ordering;

impl App {
    pub(super) fn select_activity(&mut self, work: &str) {
        self.work = work.to_owned();
        self.conversation_page = 0;
        if let Some(activity) = self.home_index.activity(work) {
            if !activity.members.contains(&self.persona) {
                self.persona = activity.members.first().cloned().unwrap_or_default();
            }
        }
    }

    pub(super) fn open_home_activity(&mut self, work: &str) {
        self.select_activity(work);
        self.layout.editor = Editor::Studio;
        self.live_board.show_activity(work);
    }

    pub(super) fn open_home_message(&mut self, work: &str, person: &str) {
        self.open_home_activity(work);
        self.persona = person.to_owned();
        self.address_in_live(person);
    }

    pub(super) fn open_home_event(&mut self, work: &str, sequence: i64) {
        let Some(event) = self
            .events
            .binary_search_by_key(&sequence, |e| e["event"].as_i64().unwrap_or(0))
            .ok()
            .map(|i| &self.events[i])
            .filter(|e| e["work"] == work)
        else {
            return;
        };
        let person = if event["invocation"].is_null()
            && matches!(s(event, "kind"), "user_message" | "human_request_progress")
        {
            s(event, "target")
        } else {
            s(event, "actor")
        }
        .to_owned();
        self.open_home_activity(work);
        if self
            .home_index
            .activity(work)
            .is_some_and(|a| a.members.contains(&person))
        {
            self.persona = person.clone();
            self.address_in_live(&person);
        }
        self.live_board.show_event(work, sequence);
    }

    pub(super) fn open_home_delivery(&mut self, reference: &str, work: &str) {
        self.select_activity(work);
        self.layout.delivery = reference.to_owned();
        self.layout.editor = Editor::Delivery;
    }

    pub(super) fn open_issued_package(&mut self, ctx: &egui::Context, reference: &str) {
        let work = self.records("deliveries").iter()
            .find(|record| inbox::handle(record) == reference)
            .and_then(|record| record["body"]["work"].as_str()).map(str::to_owned);
        if let Some(work) = work {
            self.open_home_delivery(reference, &work);
        } else {
            self.open_resource(ctx, reference.into());
        }
    }

    pub(super) fn selected_home_package(&self) -> Option<&Value> {
        self.records("deliveries")
            .iter()
            .find(|d| inbox::handle(d) == self.layout.delivery)
    }

    pub(super) fn can_start_run(&self) -> bool {
        !self.running
            && !self.busy
            && !self.snapshot.status.is_null()
            && !matches!(self.runner_observation, RunnerObservation::Observed { .. })
    }

    fn run_scope_name(&self, scope: &RunScope) -> String {
        match scope {
            RunScope::Workspace => "all activities".into(),
            RunScope::ActivityTree { work } => {
                format!("{} and its requested activities", self.work_name(work))
            }
            RunScope::Selection { work, persona } => match (work, persona) {
                (Some(w), Some(p)) => format!("{} in {}", self.label(p), self.work_name(w)),
                (Some(w), None) => self.work_name(w),
                (None, Some(p)) => self.label(p).to_owned(),
                (None, None) => "all activities".into(),
            },
        }
    }

    pub(super) fn runner_status_text(&self) -> String {
        if self.closing {
            return "Waiting for owned work to settle".into();
        }
        if self.snapshot.status.is_null() {
            return "Opening workspace…".into();
        }
        if let RunnerObservation::Observed {
            session,
            workers,
            finish_requested_by,
        } = &self.runner_observation
        {
            let scope = match &session.scope {
                RunScope::ActivityTree { work } => {
                    format!("{} + requests", excerpt(&self.work_name(work), 36))
                }
                other => excerpt(&self.run_scope_name(other), 46),
            };
            let state = if finish_requested_by.is_some()
                || self
                    .following
                    .as_ref()
                    .is_some_and(|(_, f)| f.load(Ordering::Acquire))
            {
                "Finishing"
            } else if session.watch && workers.is_empty() {
                "Listening"
            } else {
                "Running"
            };
            return format!("{state} · {scope}");
        }
        if self.running {
            return "Starting or settling run…".into();
        }
        if matches!(
            self.runner_observation,
            RunnerObservation::Unavailable { .. }
        ) {
            return "Runner observation unavailable".into();
        }
        "Workspace connected".into()
    }

    pub(super) fn finish_run_button(&mut self, ui: &mut Ui) {
        let local = self
            .following
            .as_ref()
            .map(|(_, f)| f.load(Ordering::Acquire));
        let (session, requested, description) = match &self.runner_observation {
            RunnerObservation::Observed {
                session,
                finish_requested_by,
                ..
            } => (
                Some(session.id.clone()),
                finish_requested_by.is_some(),
                self.run_scope_name(&session.scope),
            ),
            _ if local.is_some() => (None, false, "this window’s run".into()),
            _ => return,
        };
        let requested = requested || local == Some(true);
        if ui
            .add_enabled(
                !self.busy && !requested,
                egui::Button::new(if requested {
                    "Finishing…"
                } else {
                    "Finish run"
                }),
            )
            .on_hover_text(format!(
                "Finish the run for {description}. Decisions already admitted can settle."
            ))
            .clicked()
        {
            if let Some(session) = session {
                self.command(ui.ctx(), Command::FinishRun { session });
            } else {
                self.finish_following();
            }
        }
    }

    pub(super) fn workspace_run_controls(&mut self, ui: &mut Ui) {
        let observed = matches!(self.runner_observation, RunnerObservation::Observed { .. });
        if observed || self.running {
            let status = ui.label(self.runner_status_text());
            if let RunnerObservation::Observed { session, .. } = &self.runner_observation {
                status.on_hover_text(self.run_scope_name(&session.scope));
            }
            self.finish_run_button(ui);
        } else {
            let held = self.records("controls").iter().find(|c| {
                matches!(s(c, "scope"), "global" | "admission") && c["state"] != "running"
            });
            if let Some(control) = held {
                let scope = s(control, "scope").to_owned();
                let label = if scope == "global" { "Enable scheduling" } else { "Enable new calls" };
                if ui.add_enabled(!self.busy, egui::Button::new(label)).clicked() {
                    self.command(ui.ctx(), Command::Control { scope, state: "running".into(), reason: format!("Operator selected {label} in Studio") });
                }
            } else if ui.add_enabled(self.can_start_run(), theme::primary("Run workspace"))
                .on_hover_text("Listen across all activities and let each persona take its own eligible turns.").clicked()
            {
                self.start_follow(ui.ctx(), None);
            }
            if let RunnerObservation::Unavailable { reason } = &self.runner_observation {
                ui.weak("Runner status unavailable").on_hover_text(reason);
            }
        }
    }
}
