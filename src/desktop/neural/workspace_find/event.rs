//! Exact operator navigation in this workspace's recorded timeline. Do not
//! index message bodies or turn a search into a message, request or run.
use super::*;

pub(super) enum Address {
    Number(i64),
    Invalid,
}

/// Reserve canonical event handles and numeric # shortcuts in All. Ordinary
/// text (including a filename such as #2.dxf) stays a catalogue query.
impl Address {
    pub(super) fn parse(query: &str) -> Option<Self> {
        let query = query.trim();
        let digits = if let Some(tail) = query.strip_prefix("experience:event-") {
            let Some(digits) = tail.strip_suffix("@1") else {
                return Some(Self::Invalid);
            };
            digits
        } else {
            let digits = query.strip_prefix('#')?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits
        };
        Some(match digits.parse::<i64>() {
            Ok(n) if n > 0 && n.to_string() == digits => Self::Number(n),
            _ => Self::Invalid,
        })
    }

    pub(super) fn result(
        &self,
        ui: &mut Ui,
        app: &App,
        enter: bool,
    ) -> Option<(Kind, String, String)> {
        ui.separator();
        let Self::Number(seq) = self else {
            ui.weak("Use a number like #123 (no leading zeros), or an exact address like experience:event-123@1.");
            return None;
        };
        if app.snapshot.status.is_null() {
            ui.weak(if app.error.is_some() {
                "Workspace data is unavailable."
            } else {
                "Opening workspace…"
            });
            return None;
        }
        let reference = format!("experience:event-{seq}@1");
        let Some(i) = home_navigation::event_position(&app.events, &reference) else {
            ui.weak(format!("No event #{seq} in the loaded workspace timeline."));
            return None;
        };
        let event = &app.events[i];
        let work = s(event, "work");
        let Some(activity) = app.home_index.activity(work) else {
            ui.weak(format!("The activity for event #{seq} is unavailable."));
            return None;
        };
        let title = format!(
            "#{seq} · {} {}",
            app.event_author(event),
            app.event_heading(event)
        );
        let subtitle = format!("{}  ·  {}", s(event, "created_utc"), activity.title);
        let open =
            result_row(ui, Kind::Event, &reference, &title, &subtitle, true).clicked() || enter;
        ui.separator();
        ui.weak("Enter  Open at event    ·    Holds the timeline here, without resuming work.");
        open.then(|| (Kind::Event, reference, work.to_owned()))
    }
}

pub(super) fn open(app: &mut App, reference: &str, work: &str) {
    let sequence = home_navigation::event_position(&app.events, reference)
        .map(|i| &app.events[i])
        .filter(|event| event["work"] == work && app.home_index.activity(work).is_some())
        .and_then(|event| event["event"].as_i64());
    if let Some(seq) = sequence {
        // Unlike a conversation shortcut, inspecting an event is not a reply.
        app.select_activity(work);
        app.layout.editor = Editor::Studio;
        app.live_board.show_event(work, seq, app.cursor);
    } else {
        app.error = Some("This event is no longer in the loaded activity timeline.".into());
    }
}
