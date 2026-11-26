//! Hold a place in one activity's timeline, independently of new arrivals.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Hold {
    pub(super) at: i64,
    through: i64,
}

impl Hold {
    pub(super) fn new(at: i64, observed: i64) -> Self {
        Self {
            at,
            through: observed.max(at),
        }
    }

    /// Partition the filtered, ordered rows: visible / already recorded after
    /// our place / newly arrived. The counts are events, not decisions or runs.
    fn ends(self, matches: &[usize], events: &[Value]) -> (usize, usize) {
        let end =
            |seq| matches.partition_point(|&i| events[i]["event"].as_i64().unwrap_or(0) <= seq);
        (end(self.at), end(self.through))
    }
}

impl Board {
    pub(super) fn follow_control(&mut self, ui: &mut Ui, events: &[Value], cursor: i64) -> usize {
        self.index(events);
        let latest = self.matches.len();
        let (end, seen) = self
            .hold
            .map_or((latest, latest), |h| h.ends(&self.matches, events));
        let (later, new) = (seen - end, latest - seen);
        let counts = match (later, new) {
            (0, 0) => String::new(),
            (0, n) => format!("{n} new"),
            (n, 0) => format!("{n} later"),
            (l, n) => format!("{l} later · {n} new"),
        };
        let paused = self.hold.is_some();
        // A number inside the toggle can truncate the distinction between
        // later and new. Let the existing wrapping toolbar lay out both.
        let label = if paused {
            "Return to latest"
        } else {
            "Pause feed"
        };
        let toggle = ui.add_sized(
            [116., ui.spacing().interact_size.y],
            egui::Button::new(RichText::new(label).small())
                .truncate()
                .selected(paused),
        );
        let toggle = if paused {
            toggle.on_hover_text(format!("Follow the latest again. In this filter: {later} later events were already recorded; {new} arrived since you held the feed."))
        } else {
            toggle.on_hover_text("Keep your place while new events arrive")
        };
        if toggle.clicked() {
            toggle.request_focus();
            if paused {
                self.hold = None;
                self.jump_to_latest = true;
                return latest;
            }
            self.hold = Some(Hold::new(cursor, cursor));
        }
        if !counts.is_empty() {
            ui.label(
                RichText::new(counts)
                    .small()
                    .color(theme::palette(ui).ink(theme::ACCENT)),
            );
        }
        if let (Some(hold), Some(&i)) = (self.hold, self.matches.get(end)) {
            if ui
                .small_button("Next event →")
                .on_hover_text("Read the next event in this filter. Keep the feed held.")
                .clicked()
            {
                if let Some(next) = events[i]["event"].as_i64() {
                    self.hold = Some(Hold::new(next, hold.through));
                    self.expanded = Some(next);
                    self.jump_to_latest = true;
                    return end + 1;
                }
            }
        }
        end
    }
}
