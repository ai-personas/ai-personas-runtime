use super::*;

fn revision(reference: &str) -> &str {
    reference.rsplit_once('@').map_or("?", |(_, n)| n)
}

fn verb(tool: &str) -> &'static str {
    match tool {
        "asset.save" => "Saved",
        "asset.organize" => "Organized",
        "asset.register" => "Registered",
        "asset.import" => "Imported",
        _ => "Retained",
    }
}

fn link(ui: &mut Ui, label: &str, target: Option<&str>, action: &mut Option<Action>) {
    if let Some(target) = target {
        if ui.link(label).on_hover_text(target).clicked() {
            *action = Some(Action::Open(Target::File(target.into())));
        }
    }
}

impl App {
    pub(super) fn file_history_contents(&mut self, ui: &mut Ui, action: &mut Option<Action>) {
        let viewport = ui.ctx().content_rect();
        let width = (viewport.width() - 52.).clamp(140., 1080.);
        ui.set_width(width);
        let narrow = width < 790.;
        let p = theme::palette(ui);
        let history = &mut self.file_history;
        ui.horizontal(|ui| {
            theme::glyph(ui, Icon::Files, p.accent);
            ui.label(RichText::new("File history").size(20.).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    ui.close();
                }
            });
        });
        ui.add_space(6.);
        let name = if history.opened_path.is_empty() {
            &history.opened
        } else {
            &history.opened_path
        };
        ui.add(
            egui::Label::new(
                RichText::new(name)
                    .size(if narrow { 20. } else { 24. })
                    .strong(),
            )
            .wrap(),
        );
        let loaded = history.page.result.clone();
        ui.horizontal_wrapped(|ui| {
            let mut focus = |ui: &mut Ui, label: String, r: &str| {
                if r == history.selected || history.page.pending.is_some() {
                    ui.weak(label).on_hover_text(r);
                } else if ui.link(label).on_hover_text(r).clicked() {
                    *action = Some(Action::Select(r.into()));
                }
            };
            focus(
                ui,
                format!("Opened revision {}", revision(&history.opened)),
                &history.opened,
            );
            if let Some(Ok(page)) = &loaded {
                match page["current_reference"].as_str() {
                    Some(current) => {
                        ui.weak("·");
                        focus(ui, format!("Current head {}", revision(current)), current);
                    }
                    None => {
                        ui.weak("·  Current head unavailable");
                    }
                }
                if let Some(total) = page["total"].as_u64() {
                    ui.weak(format!(
                        "·  {}",
                        counted(total, "retained revision", "retained revisions")
                    ));
                }
            }
            if history.page.pending.is_some() {
                ui.add(egui::Spinner::new().size(13.));
            }
        });
        ui.add_space(4.);
        ui.separator();
        let page = match loaded {
            None => {
                ui.weak("Reading retained revisions…");
                return;
            }
            Some(Err(reason)) => {
                ui.strong("File history unavailable");
                ui.add(egui::Label::new(reason).wrap());
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Retry history").clicked() {
                        *action = Some(Action::RetryHistory);
                    }
                    link(ui, "Open exact file", Some(&history.opened), action);
                });
                return;
            }
            Some(Ok(page)) => page,
        };
        if narrow {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut history.show_list,
                    false,
                    format!("Revision {}", revision(&history.selected)),
                );
                ui.selectable_value(&mut history.show_list, true, "All revisions");
            });
        }
        let height = (viewport.height() - if narrow { 230. } else { 202. }).clamp(95., 580.);
        let list = |app: &mut App, ui: &mut Ui, action: &mut Option<Action>| {
            egui::ScrollArea::vertical()
                .id_salt(("file-history-list", &app.file_history.opened))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| app.revision_list(ui, &page, action));
        };
        let detail = |app: &mut App, ui: &mut Ui, action: &mut Option<Action>| {
            let selected = app.file_history.selected.clone();
            egui::ScrollArea::vertical()
                .id_salt(("file-history-edition", &selected))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(row) = rows(&page).iter().find(|r| r["reference"] == selected) {
                        app.revision_detail(ui, row, action);
                    } else if app.file_history.page.pending.is_some() {
                        ui.weak("Locating this revision…");
                    } else {
                        ui.weak("This page has no retained file revisions.");
                    }
                });
        };
        if narrow {
            if history.show_list {
                list(self, ui, action);
            } else {
                detail(self, ui, action);
            }
        } else {
            ui.horizontal_top(|ui| {
                let left = (width * 0.28).clamp(220., 280.);
                ui.allocate_ui_with_layout(
                    egui::vec2(left, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(left);
                        list(self, ui, action);
                    },
                );
                ui.separator();
                let right = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(right, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(right);
                        detail(self, ui, action);
                    },
                );
            });
        }
    }

    fn revision_list(&self, ui: &mut Ui, page: &Value, action: &mut Option<Action>) {
        ui.strong("Retained revisions");
        let total = page["total"].as_u64().unwrap_or(0) as usize;
        let offset = page["offset"].as_u64().unwrap_or(0) as usize;
        if total > PAGE {
            ui.horizontal_wrapped(|ui| {
                let ready = self.file_history.page.pending.is_none();
                if ui
                    .add_enabled(ready && offset > 0, egui::Button::new("Newer").small())
                    .clicked()
                {
                    *action = Some(Action::Page(offset.saturating_sub(PAGE)));
                }
                ui.small(format!(
                    "{}–{} of {total}",
                    offset + 1,
                    (offset + PAGE).min(total)
                ));
                if ui
                    .add_enabled(
                        ready && offset + PAGE < total,
                        egui::Button::new("Older").small(),
                    )
                    .clicked()
                {
                    *action = Some(Action::Page(offset + PAGE));
                }
            });
        }
        for row in rows(page) {
            let reference = s(row, "reference");
            ui.push_id(reference, |ui| {
                ui.add_space(5.);
                let selected = reference == self.file_history.selected;
                let p = theme::palette(ui);
                let highlight = if selected { p.selection } else { p.panel };
                let response = egui::Frame::new()
                    .fill(highlight)
                    .inner_margin(8)
                    .corner_radius(6)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(format!("Revision {}", revision(reference)));
                            if row["current"] == true {
                                ui.small("Current");
                            }
                            if reference == self.file_history.opened {
                                ui.small("Opened");
                            }
                        });
                        ui.label(match row["editor"]["person"].as_str() {
                            Some(who) => format!("{} by {}", verb(s(row, "tool")), self.label(who)),
                            None => "Editor unverified".into(),
                        });
                        if let Some(stamp) = row["created_utc"].as_str() {
                            ui.small(RichText::new(stamp).color(p.muted));
                        }
                    })
                    .response;
                let button = ui
                    .interact(response.rect, ui.id().with("select"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(reference);
                button.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        true,
                        selected,
                        format!("Revision {}", revision(reference)),
                    )
                });
                if button.has_focus() || button.hovered() {
                    ui.painter().rect_stroke(
                        response.rect,
                        6.,
                        Stroke::new(1., p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if button.clicked() {
                    *action = Some(Action::Select(reference.into()));
                }
            });
        }
    }

    fn revision_detail(&mut self, ui: &mut Ui, row: &Value, action: &mut Option<Action>) {
        let reference = s(row, "reference");
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Revision {}", revision(reference)))
                    .size(20.)
                    .strong(),
            );
            if row["retired"] == true {
                ui.small("Retired");
            }
            link(ui, "Open exact file", Some(reference), action);
        });
        if let Some(path) = row["path"].as_str() {
            if path != self.file_history.opened_path {
                ui.add(egui::Label::new(path).wrap());
            }
        }
        if row["record_verified"] != true {
            ui.weak("The retained version record could not be verified.");
        }
        ui.add_space(5.);
        let editor = &row["editor"];
        let origin = &row["content_origin"];
        let same_source = editor.is_object() && editor == origin;
        if let Some(who) = editor["person"].as_str() {
            let edit = if same_source && s(editor, "tool") == "asset.save" {
                "Saved and authored"
            } else {
                verb(s(editor, "tool"))
            };
            ui.label(RichText::new(format!("{} by {}", edit, self.label(who))).strong())
                .on_hover_text(who);
        } else {
            ui.label("Revision editor unverified");
        }
        self.file_evidence(ui, editor, "Revision", action);
        if same_source {
            if matches!(s(origin, "tool"), "asset.register" | "asset.import") {
                ui.weak("Original content author unrecorded");
            }
        } else if let Some(who) = origin["person"].as_str() {
            let role = match s(origin, "tool") {
                "asset.save" => "Content authored by",
                "asset.register" => "Content registered by",
                "asset.import" => "Content imported by",
                _ => "Content supplied by",
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{role} {}", self.label(who)))
                    .on_hover_text(who);
                if let Some(source) = origin["reference"].as_str().filter(|r| *r != reference) {
                    link(
                        ui,
                        &format!("Source revision {}", revision(source)),
                        Some(source),
                        action,
                    );
                }
            });
            if origin["decision"] != editor["decision"] || origin["receipt"] != editor["receipt"] {
                self.file_evidence(ui, origin, "Content", action);
            }
        } else {
            ui.weak("Content origin unverified");
        }
        self.file_deliveries(ui, reference, action);
        ui.add_space(6.);
        ui.separator();
        ui.add_space(3.);
        if let Some(previous) = row["previous"].as_str() {
            ui.label(
                RichText::new(format!("Changes from revision {}", revision(previous)))
                    .size(17.)
                    .strong(),
            );
            let changed = fields(row);
            if !changed.is_empty() {
                ui.small(format!("Changed metadata · {}", changed.join(", ")));
            }
            let pair = &self.file_history.comparing;
            if pair
                .as_ref()
                .is_none_or(|(a, b)| a != reference || b != previous)
            {
                ui.weak("Reading both revisions…");
            } else if let Some(result) = self.file_history.comparison.result.clone() {
                match result {
                    Ok(comparison) => {
                        show_comparison(ui, &comparison, &mut self.file_history.diff_page, action)
                    }
                    Err(reason) => {
                        ui.strong("Comparison unavailable");
                        ui.add(egui::Label::new(reason).wrap());
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Retry comparison").clicked() {
                                *action = Some(Action::RetryComparison);
                            }
                            link(ui, "Open previous file", Some(previous), action);
                            link(ui, "Open selected file", Some(reference), action);
                        });
                    }
                }
            } else {
                ui.weak("Reading both revisions…");
            }
        } else if row["changes"]["content"] == "created" {
            ui.strong("First retained revision");
            if let Some(bytes) = row["bytes"].as_u64() {
                ui.label(counted(bytes, "byte", "bytes"));
            }
        } else {
            ui.weak("No verified adjacent revision is available for comparison.");
        }
        ui.add_space(6.);
        egui::CollapsingHeader::new("Exact references")
            .id_salt(("file-history-refs", reference))
            .show(ui, |ui| {
                ui.label(RichText::new(reference).monospace())
                    .on_hover_text("Exact retained file");
                if ui.small_button("Copy file reference").clicked() {
                    ui.ctx().copy_text(reference.into());
                }
                if let Some(sha) = row["sha256"].as_str() {
                    ui.small("Verified SHA-256");
                    ui.add(egui::Label::new(RichText::new(sha).monospace()).wrap());
                }
            });
    }

    fn file_evidence(&self, ui: &mut Ui, source: &Value, label: &str, action: &mut Option<Action>) {
        if !source.is_object() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(work) = source["work"].as_str() {
                let name = self.work_name(work);
                ui.small("Activity");
                if ui
                    .link(excerpt(&name, 52))
                    .on_hover_text(format!("{name}\n{work}"))
                    .clicked()
                {
                    *action = Some(Action::Open(Target::Activity(work.into())));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (field, name) in [("decision", "decision"), ("receipt", "receipt")] {
                if let Some(exact) = source[field].as_str() {
                    if ui
                        .small_button(format!("{label} {name}"))
                        .on_hover_text(exact)
                        .clicked()
                    {
                        *action = Some(Action::Open(Target::File(exact.into())));
                    }
                }
            }
        });
    }
}

fn fields(row: &Value) -> Vec<String> {
    row["changes"]["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|k| {
            !matches!(
                *k,
                "blob"
                    | "previous_revision"
                    | "last_content_author"
                    | "content_authority"
                    | "origin_decision"
                    | "origin_operation"
                    | "organized_in_work"
                    | "organized_by"
                    | "owner"
            )
        })
        .map(|k| k.replace('_', " "))
        .collect()
}

fn show_comparison(
    ui: &mut Ui,
    comparison: &compare::Comparison,
    page: &mut usize,
    action: &mut Option<Action>,
) {
    let previous = &comparison.before;
    let current = &comparison.after;
    egui::Frame::new()
        .inner_margin(8)
        .fill(theme::palette(ui).panel)
        .corner_radius(5)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            for file in [previous, current] {
                ui.horizontal_wrapped(|ui| {
                    link(
                        ui,
                        &format!("Revision {}", revision(&file.reference)),
                        Some(&file.reference),
                        action,
                    );
                    ui.small(format!(
                        "{}  ·  {}",
                        file.format,
                        counted(file.bytes, "byte", "bytes")
                    ));
                    if previous.path != current.path {
                        ui.small(&file.path);
                    }
                });
            }
        });
    ui.add_space(3.);
    match &comparison.content {
        compare::Content::Identical => {
            ui.label("File bytes are identical.");
        }
        compare::Content::Summary(reason) => {
            ui.add(egui::Label::new(*reason).wrap());
        }
        compare::Content::Region {
            before,
            after,
            before_count,
            after_count,
        } => {
            ui.label("The differing region is too large to align inline. These excerpts start after the common prefix.");
            for (file, lines, n) in [
                (previous, before, before_count),
                (current, after, after_count),
            ] {
                ui.small(format!(
                    "Revision {} · first {} of {} lines in the region",
                    revision(&file.reference),
                    lines.len(),
                    n
                ));
                for line in lines {
                    source_line(ui, line, false);
                }
            }
        }
        compare::Content::Lines {
            rows,
            added,
            removed,
            shortened,
        } => {
            ui.label(format!(
                "{} added  ·  {} removed",
                counted(*added as u64, "line", "lines"),
                counted(*removed as u64, "line", "lines")
            ));
            if *shortened {
                ui.small(
                    "Long lines are shortened here. Both complete revisions are available above.",
                );
            }
            const CHUNK: usize = 24;
            *page = (*page).min(rows.len().saturating_sub(1) / CHUNK);
            if rows.len() > CHUNK {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(*page > 0, egui::Button::new("Previous lines").small())
                        .clicked()
                    {
                        *page -= 1;
                    }
                    ui.small(format!(
                        "{}–{} of {} lines shown",
                        *page * CHUNK + 1,
                        ((*page + 1) * CHUNK).min(rows.len()),
                        rows.len()
                    ));
                    if ui
                        .add_enabled(
                            (*page + 1) * CHUNK < rows.len(),
                            egui::Button::new("Next lines").small(),
                        )
                        .clicked()
                    {
                        *page += 1;
                    }
                });
            }
            for line in rows.iter().skip(*page * CHUNK).take(CHUNK) {
                if line.gap {
                    ui.separator();
                }
                source_line(ui, line, true);
            }
        }
    }
}

fn source_line(ui: &mut Ui, line: &compare::Line, edits: bool) {
    let p = theme::palette(ui);
    let kind = if !edits || line.new.is_some() && line.old.is_some() {
        0
    } else if line.old.is_none() {
        1
    } else {
        -1
    };
    let dark = ui.visuals().dark_mode;
    let (color, mark) = match kind {
        1 => (
            if dark {
                Color32::from_rgb(128, 214, 175)
            } else {
                Color32::from_rgb(21, 105, 65)
            },
            "+",
        ),
        -1 => (
            if dark {
                Color32::from_rgb(242, 166, 151)
            } else {
                Color32::from_rgb(147, 54, 44)
            },
            "−",
        ),
        _ => (p.muted, " "),
    };
    ui.horizontal_top(|ui| {
        let number = |n: Option<usize>| n.map(|n| n.to_string()).unwrap_or_default();
        let numbers = format!("{:>3} {:>3} {}", number(line.old), number(line.new), mark);
        ui.label(RichText::new(numbers).monospace().small().color(color))
            .on_hover_text("Previous line · selected line · change");
        ui.add(
            egui::Label::new(
                RichText::new(&line.text)
                    .monospace()
                    .size(13.)
                    .color(if kind == 0 { p.text } else { color }),
            )
            .wrap()
            .selectable(true),
        );
    });
}
