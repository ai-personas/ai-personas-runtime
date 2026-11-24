use super::*;

#[test]
fn settings_fit_after_zoom_and_viewport_changes() {
    let directory = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, directory.path().to_path_buf(), None, "Evidence contributor".into());
    app.open_workspace_settings();
    // Retain the same window identity across zoom/resize transitions, as in the native regression.
    for (pixels, scale) in [(egui::vec2(1440.,900.),1.),(egui::vec2(1440.,900.),3.),
        (egui::vec2(900.,650.),3.),(egui::vec2(780.,560.),3.),(egui::vec2(780.,560.),1.)] {
        app.layout.scale = scale;
        theme::apply(&ctx, app.layout.dark, scale);
        for _ in 0..5 {
            let input = egui::RawInput {
                // The native integration supplies logical points, after UI zoom.
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO,pixels/scale)),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.dialogs(ctx));
        }
        let screen = ctx.content_rect();
        assert!((screen.width()-pixels.x/scale).abs() < 1.,
            "The regression must exercise the actual scaled viewport: {screen:?}, {pixels:?}, {scale}");
        let rect = ctx.memory(|memory| memory.area_rect(egui::Id::new("studio-dialog"))).unwrap();
        assert!(rect.left() >= screen.left()-1. && rect.right() <= screen.right()+1.,
            "Settings header must fit for {pixels:?} at {scale}: {rect:?} outside {screen:?}");
        assert!(rect.top() >= screen.top()-1., "Settings header must remain reachable");
        assert!(app.dialog.is_some(), "Layout changes must preserve the open settings draft");
    }
}

#[test]
fn selected_person_inspector_fits_scaled_viewport() {
    let directory = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, directory.path().to_path_buf(), None, "Reviewer".into());
    app.persona = "persona-example".into();
    app.snapshot.status = serde_json::json!({"personas":[{"body":{
        "id":"persona-example", "label":"Mira", "lifecycle":"active",
        "provider":{"model":"gpt-6.1-sol","effort":"high"}
    }}]});
    app.inspector_window = true;
    for (pixels,scale) in [(egui::vec2(1440.,900.),1.), (egui::vec2(780.,560.),3.), (egui::vec2(900.,650.),3.)] {
        theme::apply(&ctx, app.layout.dark, scale);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO,pixels/scale);
        for _ in 0..5 {
            let _ = ctx.run(egui::RawInput { screen_rect:Some(screen), ..Default::default() }, |ctx| app.show_inspector_window(ctx));
        }
        let rect = ctx.memory(|m| m.area_rect(egui::Id::new("Inspector"))).unwrap();
        assert!(rect.left()>=-1. && rect.right()<=screen.right()+1. && rect.top()>=-1. && rect.bottom()<=screen.bottom()+1.,
            "Selected-person inspector must fit {screen:?}, got {rect:?}");
        // The same inspector retains its scroll offset across zoom changes.
        // Return to the beginning before scanning down for the control.
        for _ in 0..5 {
            let _ = ctx.run(egui::RawInput { screen_rect:Some(screen), events:vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::MouseWheel { unit:egui::MouseWheelUnit::Point, delta:egui::vec2(0.,10000.), modifiers:Default::default() },
            ], ..Default::default() }, |ctx| app.show_inspector_window(ctx));
        }
        let mut reasoning_reachable = false;
        for _ in 0..25 {
            let output = ctx.run(egui::RawInput { screen_rect:Some(screen), events:vec![
                egui::Event::PointerMoved(rect.center()),
                egui::Event::MouseWheel { unit:egui::MouseWheelUnit::Point, delta:egui::vec2(0.,-25.), modifiers:Default::default() },
            ], ..Default::default() }, |ctx| app.show_inspector_window(ctx));
            reasoning_reachable |= output.shapes.iter().any(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text()=="Change reasoning settings…" => {
                    let bounds=text.galley.rect.translate(text.pos.to_vec2());
                    shape.clip_rect.contains_rect(bounds) && screen.contains_rect(bounds)
                },
                _ => false,
            });
        }
        assert!(reasoning_reachable,"Reasoning controls must be scroll-reachable at {scale}x");
        assert!(app.inspector_window);
        assert_eq!(app.persona,"persona-example");
        assert!(app.dialog.is_none(), "Layout must not open a configuration action");
    }
}

#[test]
fn action_dialogs_fit_and_scroll_to_submit_at_high_zoom() {
    let reason = "Keep this attributed draft intact. ".repeat(40);
    let drafts = [
        (Dialog::Review { reference: "delivery:delivery-dcdb85f7d352d64ce2091a59340de5e2@1".into(), title: "Integrated drawings and supporting schedules for concept review".into(), reason: reason.clone() }, "Request changes"),
        (Dialog::Persona { name: "New colleague".into(), seed: reason.clone(), provider: String::new() }, "Create persona"),
        (Dialog::ActivityState { work: "work-example".into(), expected: "activity:example@1".into(), reopen: false, reason: reason.clone() }, "End activity"),
        (Dialog::Reasoning { persona: "persona-example".into(), expected: "persona:example@1".into(), model: "gpt-6.1-sol".into(), effort: "high".into(), reason: reason.clone() }, "Save reasoning settings"),
    ];
    for (draft, submit) in drafts {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, directory.path().to_path_buf(), None, "Contributor".into());
        app.dialog = Some(draft);
        for (pixels, scale) in [(egui::vec2(1440.,900.),1.), (egui::vec2(780.,560.),3.), (egui::vec2(900.,650.),3.)] {
            theme::apply(&ctx, app.layout.dark, scale);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, pixels/scale);
            for _ in 0..5 {
                let _ = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| app.dialogs(ctx));
            }
            let rect = ctx.memory(|m| m.area_rect(egui::Id::new("studio-dialog"))).unwrap();
            assert!(rect.left() >= -1. && rect.right() <= screen.right()+1. && rect.top() >= -1. && rect.bottom() <= screen.bottom()+1.,
                "{submit}: dialog must fit {screen:?}, got {rect:?}");
            let mut reachable = false;
            for _ in 0..5 {
                let output = ctx.run(egui::RawInput { screen_rect: Some(screen), events: vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.,-10000.), modifiers: Default::default() },
                ], ..Default::default() }, |ctx| app.dialogs(ctx));
                reachable |= output.shapes.iter().any(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == submit => {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        shape.clip_rect.contains_rect(bounds) && screen.contains_rect(bounds)
                    },
                    _ => false,
                });
            }
            assert!(reachable, "{submit} must be reachable by scrolling at {scale}x");
            let retained = match app.dialog.as_ref().unwrap() {
                Dialog::Persona { seed, .. } => seed,
                Dialog::ActivityState { reason, .. } | Dialog::Reasoning { reason, .. } | Dialog::Review { reason, .. } => reason,
                _ => unreachable!(),
            };
            assert_eq!(retained, &reason, "Resizing/scrolling must not discard draft text");
        }
    }
}
