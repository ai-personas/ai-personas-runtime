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
