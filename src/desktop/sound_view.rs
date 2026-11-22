//! Time-aligned views of PCM audio and Standard MIDI files.
use super::{design, theme};
use eframe::egui::{self, Color32, Rect, Stroke, Ui};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Read,
};

pub(super) struct Wave {
    peaks: Vec<(f32, f32)>,
    duration: f64,
    channels: u16,
    rate: u32,
}
impl Wave {
    pub fn read(
        reader: impl Read,
        cancel: &super::native_preview::Cancellation,
    ) -> Result<Self, String> {
        let mut wav = hound::WavReader::new(reader).map_err(|e| e.to_string())?;
        let spec = wav.spec();
        let length = wav.len() as usize;
        let mut peaks = vec![(0.0_f32, 0.0_f32); 1200.min(length.max(1))];
        let last = peaks.len();
        let mut record = |i: usize, v: f32| {
            if i % 16384 == 0 && cancel.is_cancelled() {
                return Err("Preview cancelled.".to_owned());
            }
            let index = (i as u64 * last as u64 / length.max(1) as u64) as usize;
            let bucket = &mut peaks[index.min(last - 1)];
            bucket.0 = bucket.0.min(v.clamp(-1., 1.));
            bucket.1 = bucket.1.max(v.clamp(-1., 1.));
            Ok(())
        };
        if spec.sample_format == hound::SampleFormat::Float {
            for (i, value) in wav.samples::<f32>().enumerate() {
                record(i, value.map_err(|e| e.to_string())?)?;
            }
        } else {
            let scale = 2_f32.powi(i32::from(spec.bits_per_sample) - 1);
            for (i, value) in wav.samples::<i32>().enumerate() {
                record(i, value.map_err(|e| e.to_string())? as f32 / scale)?;
            }
        }
        Ok(Self {
            peaks,
            duration: wav.duration() as f64 / f64::from(spec.sample_rate),
            channels: spec.channels,
            rate: spec.sample_rate,
        })
    }

    pub fn ui(&self, ui: &mut Ui) {
        let p = theme::palette(ui);
        ui.label(format!(
            "{:.2} seconds  ·  {} Hz  ·  {} channels",
            self.duration, self.rate, self.channels
        ));
        ui.add_space(8.);
        let (r, _) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                (ui.available_height() - 30.).clamp(60., 300.),
            ),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(r, 8, p.field);
        let plot = r.shrink(15.);
        for i in 0..=4 {
            let x = plot.left() + plot.width() * i as f32 / 4.;
            ui.painter()
                .vline(x, plot.y_range(), Stroke::new(1., p.border));
        }
        ui.painter()
            .hline(plot.x_range(), plot.center().y, Stroke::new(1., p.border));
        for (i, &(low, high)) in self.peaks.iter().enumerate() {
            let x = plot.left() + plot.width() * i as f32 / self.peaks.len() as f32;
            ui.painter().line_segment(
                [
                    egui::pos2(x, plot.center().y - high * plot.height() / 2.),
                    egui::pos2(x, plot.center().y - low * plot.height() / 2.),
                ],
                Stroke::new((plot.width() / self.peaks.len() as f32).max(1.), p.accent),
            );
        }
        ui.horizontal(|ui| {
            ui.small("0 s");
            design::action_at_right(ui, |ui| {
                ui.small(format!("{:.2} s", self.duration));
            });
        });
    }
}

struct Note {
    start: u64,
    end: u64,
    pitch: u8,
    track: usize,
}
pub(super) struct Score {
    notes: Vec<Note>,
    tracks: Vec<String>,
    duration: u64,
    unit: String,
}
impl Score {
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        use midly::{MetaMessage, MidiMessage, Smf, TrackEventKind};
        let midi = Smf::parse(bytes).map_err(|e| e.to_string())?;
        let mut score = Self {
            notes: Vec::new(),
            tracks: Vec::new(),
            duration: 0,
            unit: match midi.header.timing {
                midly::Timing::Metrical(t) => format!("{} ticks / quarter note", t.as_int()),
                midly::Timing::Timecode(fps, sub) => {
                    format!("{} fps · {sub} ticks / frame", fps.as_f32())
                }
            },
        };
        for (track_index, track) in midi.tracks.iter().enumerate() {
            let mut name = format!("Track {}", track_index + 1);
            let mut starts: BTreeMap<(u8, u8), VecDeque<u64>> = BTreeMap::new();
            let mut tick = 0u64;
            for event in track {
                tick += u64::from(event.delta.as_int());
                match event.kind {
                    TrackEventKind::Meta(MetaMessage::TrackName(value)) if !value.is_empty() => {
                        name = String::from_utf8_lossy(value).into_owned()
                    }
                    TrackEventKind::Midi { channel, message } => match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => starts
                            .entry((channel.as_int(), key.as_int()))
                            .or_default()
                            .push_back(tick),
                        MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                            if let Some(start) = starts
                                .get_mut(&(channel.as_int(), key.as_int()))
                                .and_then(VecDeque::pop_front)
                            {
                                score.notes.push(Note {
                                    start,
                                    end: tick,
                                    pitch: key.as_int(),
                                    track: track_index,
                                });
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            for ((_, pitch), held) in starts {
                for start in held {
                    score.notes.push(Note {
                        start,
                        end: tick,
                        pitch,
                        track: track_index,
                    });
                }
            }
            score.tracks.push(name);
            score.duration = score.duration.max(tick);
        }
        Ok(score)
    }

    pub fn ui(&self, ui: &mut Ui) {
        let p = theme::palette(ui);
        ui.label(format!(
            "{} notes  ·  {} tracks  ·  {}",
            self.notes.len(),
            self.tracks.len(),
            self.unit
        ));
        if self.notes.is_empty() {
            ui.weak("No note events in this sequence.");
            return;
        }
        let colors = [
            Color32::from_rgb(188, 101, 70),
            Color32::from_rgb(56, 132, 126),
            Color32::from_rgb(125, 102, 166),
            Color32::from_rgb(67, 109, 156),
        ];
        let low = self
            .notes
            .iter()
            .map(|n| n.pitch)
            .min()
            .unwrap()
            .saturating_sub(2);
        let high = self
            .notes
            .iter()
            .map(|n| n.pitch)
            .max()
            .unwrap()
            .saturating_add(2)
            .min(127);
        let rows = u16::from(high) - u16::from(low) + 1;
        ui.add_space(6.);
        let (r, response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                (ui.available_height() - 90.).clamp(100., 420.),
            ),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(r, 7, p.panel);
        let plot = Rect::from_min_max(r.min + egui::vec2(35., 8.), r.max - egui::vec2(8., 8.));
        let height = plot.height() / f32::from(rows);
        let y = |pitch: u8| plot.top() + f32::from(high - pitch) * height;
        for pitch in low..=high {
            if matches!(pitch % 12, 1 | 3 | 6 | 8 | 10) {
                let stripe = Rect::from_min_size(
                    egui::pos2(plot.left(), y(pitch)),
                    egui::vec2(plot.width(), height),
                );
                ui.painter().rect_filled(stripe, 0, p.header);
            }
            if pitch % 12 == 0 {
                ui.painter().text(
                    egui::pos2(plot.left() - 4., y(pitch) + height / 2.),
                    egui::Align2::RIGHT_CENTER,
                    format!("C{}", i16::from(pitch) / 12 - 1),
                    egui::FontId::monospace(10.),
                    p.muted,
                );
            }
        }
        for i in 0..=8 {
            ui.painter().vline(
                plot.left() + i as f32 * plot.width() / 8.,
                plot.y_range(),
                Stroke::new(1., p.border),
            );
        }
        let x = |tick: u64| {
            plot.left() + (tick as f64 / self.duration.max(1) as f64) as f32 * plot.width()
        };
        let mut hover = None;
        let pointer = response.hover_pos();
        for n in &self.notes {
            let start = x(n.start);
            let end = x(n.end).max(start + 1.);
            let rect = Rect::from_min_max(
                egui::pos2(start, y(n.pitch) + 0.5),
                egui::pos2(end, y(n.pitch) + (height - 0.5).max(1.)),
            );
            ui.painter()
                .rect_filled(rect, 2, colors[n.track % colors.len()]);
            if pointer.is_some_and(|pos| rect.contains(pos)) {
                hover = Some(n);
            }
        }
        if let Some(n) = hover {
            response.on_hover_text(format!(
                "{} · MIDI note {} · ticks {}–{}",
                self.tracks[n.track], n.pitch, n.start, n.end
            ));
        }
        ui.horizontal_wrapped(|ui| {
            for (index, track) in self.tracks.iter().enumerate() {
                ui.colored_label(p.ink(colors[index % colors.len()]), format!("●  {track}"));
            }
        });
    }
}
