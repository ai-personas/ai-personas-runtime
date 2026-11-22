use super::{native_preview::Cancellation, Body};
use eframe::egui::{self, Ui};
use image::AnimationDecoder;
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

pub(super) struct Sequence {
    frames: Vec<(egui::ColorImage, Duration)>,
    info: String,
}

pub(super) struct Player {
    index: usize,
    playing: bool,
    next: Option<Instant>,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            index: 0,
            playing: true,
            next: None,
        }
    }
}

pub(super) fn animated(
    bytes: &[u8],
    ext: &str,
    cancel: &Cancellation,
) -> Result<Option<Body>, String> {
    let read = Cursor::new(bytes);
    let frames = match ext {
        "gif" => image::codecs::gif::GifDecoder::new(read)
            .map_err(|e| e.to_string())?
            .into_frames(),
        "webp" => {
            let decoder = image::codecs::webp::WebPDecoder::new(read).map_err(|e| e.to_string())?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            decoder.into_frames()
        }
        "png" => {
            let decoder = image::codecs::png::PngDecoder::new(read).map_err(|e| e.to_string())?;
            if !decoder.is_apng().map_err(|e| e.to_string())? {
                return Ok(None);
            }
            decoder.apng().map_err(|e| e.to_string())?.into_frames()
        }
        _ => return Ok(None),
    };
    let mut images = Vec::new();
    let mut memory = 0;
    let mut limited = false;
    for frame in frames {
        if cancel.is_cancelled() {
            return Err("Preview cancelled.".into());
        }
        let frame = frame.map_err(|e| e.to_string())?;
        let (ms, denom) = frame.delay().numer_denom_ms();
        let rgba = frame.into_buffer();
        memory += rgba.len();
        if !images.is_empty() && (memory > 96 * 1024 * 1024 || images.len() >= 1200) {
            limited = true;
            break;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        images.push((
            image,
            Duration::from_secs_f64((f64::from(ms) / f64::from(denom) / 1000.).clamp(0.016, 3600.)),
        ));
    }
    let Some((first, _)) = images.first() else {
        return Err("This image contains no frames.".into());
    };
    let dimensions = format!("{} × {} px", first.size[0], first.size[1]);
    if limited || images.len() == 1 {
        let text = if limited {
            format!("{dimensions} · First frame (animation exceeds the frame cache)")
        } else {
            dimensions
        };
        return Ok(Some(Body::Image(images.swap_remove(0).0, text)));
    }
    Ok(Some(Body::Animation(Sequence {
        frames: images,
        info: dimensions,
    })))
}

impl Sequence {
    pub fn ui(
        &self,
        ui: &mut Ui,
        player: &mut Player,
        texture: &mut Option<egui::TextureHandle>,
        zoom: &mut f32,
        pan: &mut egui::Vec2,
        white: &mut bool,
    ) {
        let previous = player.index;
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if player.playing { "Pause" } else { "Play" })
                .clicked()
            {
                player.playing = !player.playing;
                player.next = None;
            }
            ui.label("Frame");
            let mut frame = player.index + 1;
            if ui
                .add(egui::DragValue::new(&mut frame).range(1..=self.frames.len()))
                .changed()
            {
                player.index = frame - 1;
                player.playing = false;
                player.next = None;
            }
            ui.weak(format!("of {}", self.frames.len()));
        });
        let now = Instant::now();
        if player.playing {
            if player.next.is_some_and(|t| now >= t) {
                player.index = (player.index + 1) % self.frames.len();
                player.next = None;
            }
            let deadline = *player.next.get_or_insert(now + self.frames[player.index].1);
            ui.ctx()
                .request_repaint_after(deadline.saturating_duration_since(now));
        }
        if previous != player.index || texture.is_none() {
            let image = self.frames[player.index].0.clone();
            if let Some(t) = texture {
                t.set(image, egui::TextureOptions::LINEAR);
            } else {
                *texture = Some(ui.ctx().load_texture(
                    "animated-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        super::image_preview(ui, texture.as_ref(), zoom, pan, white, &self.info);
    }
}
