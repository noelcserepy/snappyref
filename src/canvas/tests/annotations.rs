use super::*;

#[test]
fn textbox_rect_enforces_minimum_size() {
    let rect = normalized_textbox_rect(
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::pos2(32.0, 56.0),
    );

    assert_eq!(rect.min, eframe::egui::pos2(20.0, 40.0));
    assert_eq!(rect.width(), 240.0);
    assert_eq!(rect.height(), 56.0);
}

#[test]
fn textbox_click_draft_uses_default_note_size() {
    let pending = pending_annotation_from_draft(AnnotationDraftState::new(
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(20.0, 40.0),
    ))
    .unwrap();

    assert_eq!(pending.start_world(), eframe::egui::pos2(20.0, 40.0));
    assert_eq!(pending.end_world(), eframe::egui::pos2(260.0, 96.0));
}

#[test]
fn textbox_multiline_content_requires_more_than_min_height() {
    assert!(textbox_required_world_height(80.0, 1.0) > 56.0);

    eframe::egui::__run_test_ui(|ui| {
        let mut text = "First line\nSecond line\nThird line\nFourth line\nFifth line".to_owned();
        let output = eframe::egui::TextEdit::multiline(&mut text)
            .desired_width(80.0)
            .font(eframe::egui::FontId::proportional(24.0))
            .show(ui);

        assert!(output.galley.rows.len() >= 5);
    });
}

#[test]
fn larger_text_sizes_require_more_textbox_height() {
    assert!(textbox_font_size(CanvasTextSize::Xl) > textbox_font_size(CanvasTextSize::S));
    assert!(textbox_font_size(CanvasTextSize::M) > textbox_font_size(CanvasTextSize::S));
    assert!(textbox_font_size(CanvasTextSize::L) > textbox_font_size(CanvasTextSize::M));
    assert!(textbox_font_size(CanvasTextSize::Xl) > textbox_font_size(CanvasTextSize::L));
}

#[test]
fn textbox_content_rect_respects_available_bounds() {
    let textbox_rect = eframe::egui::Rect::from_min_size(
        eframe::egui::pos2(10.0, 20.0),
        eframe::egui::vec2(8.0, 6.0),
    );
    let content_rect = textbox_content_rect(textbox_rect);

    assert!(textbox_rect.contains(content_rect.min));
    assert!(textbox_rect.contains(content_rect.max));
    assert!(content_rect.width() <= textbox_rect.width());
    assert!(content_rect.height() <= textbox_rect.height());
}

#[test]
fn textbox_content_rect_stays_inside_zoomed_out_textbox() {
    let mut state = CanvasState::default();
    state.zoom = MIN_CANVAS_ZOOM;
    let canvas_rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(0.0, 0.0),
        eframe::egui::pos2(800.0, 600.0),
    );
    let annotation = CanvasAnnotation::new(
        11,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::pos2(260.0, 96.0),
        "zoomed out".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    );

    let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
    let content_rect = textbox_content_rect(textbox_rect);

    assert!(textbox_rect.contains(content_rect.min));
    assert!(textbox_rect.contains(content_rect.max));
    assert!(content_rect.width() <= textbox_rect.width());
    assert!(content_rect.height() <= textbox_rect.height());
}

#[test]
fn textbox_toolbar_aligns_to_content_start_when_space_allows() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(960.0, 640.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            71,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(80.0, 200.0),
            eframe::egui::pos2(420.0, 320.0),
            "toolbar".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(71);

        let annotation = state.annotation_by_id(71).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

        assert!((toolbar_rect.left() - textbox_rect.left()).abs() < 0.1);
    });
}

#[test]
fn textbox_toolbar_preserves_gap_above_when_space_allows() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(960.0, 640.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            74,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(80.0, 200.0),
            eframe::egui::pos2(420.0, 320.0),
            "toolbar".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(74);

        let annotation = state.annotation_by_id(74).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

        assert!(
            (textbox_rect.top() - toolbar_rect.bottom() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE).abs()
                < 0.1
        );
    });
}

#[test]
fn textbox_toolbar_clamps_to_top_margin_when_above_gap_does_not_fit() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(960.0, 640.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            72,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(80.0, -300.0),
            eframe::egui::pos2(420.0, -180.0),
            "toolbar".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(72);

        let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

        assert!(
            (toolbar_rect.top() - (canvas_rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN))
                .abs()
                < 0.1
        );
    });
}

#[test]
fn textbox_toolbar_clamps_right_without_unnecessary_shift() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(420.0, 320.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            73,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(220.0, 140.0),
            eframe::egui::pos2(460.0, 240.0),
            "toolbar".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(73);

        let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

        assert!(
            toolbar_rect.right()
                <= canvas_rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN + 0.1
        );
        assert!(
            toolbar_rect.left()
                >= canvas_rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - 0.1
        );
    });
}

#[test]
fn textbox_layout_cache_reuses_entry_for_identical_inputs() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let first = state.cached_textbox_layout(
            ui,
            1,
            "cache me",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
        );
        let second = state.cached_textbox_layout(
            ui,
            1,
            "cache me",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
        );

        assert!(Arc::ptr_eq(&first.galley, &second.galley));
        assert_eq!(first.key, second.key);
    });
}

#[test]
fn textbox_layout_cache_replaces_entry_when_text_changes() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let first = state.cached_textbox_layout(
            ui,
            1,
            "before",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
        );
        let second = state.cached_textbox_layout(
            ui,
            1,
            "after",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
        );

        assert!(!Arc::ptr_eq(&first.galley, &second.galley));
        assert_ne!(first.key, second.key);
    });
}

#[test]
fn textbox_layout_cache_replaces_entry_when_zoom_changes() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let first = state.cached_textbox_layout(
            ui,
            1,
            "zoom",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
        );
        let second = state.cached_textbox_layout(
            ui,
            1,
            "zoom",
            default_text_size(),
            default_annotation_color(),
            120.0,
            2.0,
        );

        assert!(!Arc::ptr_eq(&first.galley, &second.galley));
        assert_ne!(first.key, second.key);
    });
}

#[test]
fn textbox_editor_writeback_replaces_cached_layout_entry() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let textbox_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(240.0, 56.0),
        );
        let text_rect = textbox_content_rect(textbox_rect);
        let mut text = "updated text".to_owned();
        let mut layouter =
            |ui: &eframe::egui::Ui, text: &dyn eframe::egui::TextBuffer, width: f32| {
                layout_textbox_galley(
                    ui,
                    text.as_str(),
                    default_text_size(),
                    default_annotation_color(),
                    width,
                    1.0,
                )
            };

        state.cache_textbox_layout(
            9,
            "stale",
            default_text_size(),
            default_annotation_color(),
            text_rect.width(),
            1.0,
            layout_textbox_galley(
                ui,
                "stale",
                default_text_size(),
                default_annotation_color(),
                text_rect.width(),
                1.0,
            ),
        );

        let output = ui
            .allocate_ui(text_rect.size(), |ui| {
                eframe::egui::TextEdit::multiline(&mut text)
                    .desired_width(text_rect.width())
                    .layouter(&mut layouter)
                    .show(ui)
            })
            .inner;
        let entry = state.cache_textbox_layout(
            9,
            &text,
            default_text_size(),
            default_annotation_color(),
            text_rect.width(),
            1.0,
            output.galley,
        );

        assert_eq!(
            entry.key,
            textbox_layout_cache_key(
                &text,
                default_text_size(),
                default_annotation_color(),
                text_rect.width(),
                1.0,
            )
        );
        assert_eq!(
            state.render.textbox_layout_cache.get(&9).unwrap().key,
            textbox_layout_cache_key(
                &text,
                default_text_size(),
                default_annotation_color(),
                text_rect.width(),
                1.0,
            )
        );
    });
}

#[test]
fn textbox_move_applies_pointer_delta() {
    let moved = moved_textbox_rect(
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        eframe::egui::pos2(100.0, 100.0),
        eframe::egui::pos2(130.0, 145.0),
    );

    assert_eq!(moved.min, eframe::egui::pos2(50.0, 85.0));
    assert_eq!(moved.max, eframe::egui::pos2(290.0, 141.0));
}

#[test]
fn textbox_text_interactive_rect_matches_galley_bounds_for_non_empty_text() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotation = CanvasAnnotation::new(
            14,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 160.0),
            "short".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        );

        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let content_rect = textbox_content_rect(textbox_rect);
        let layout = state.cached_textbox_layout(
            ui,
            annotation.id,
            &annotation.text,
            default_text_size(),
            default_annotation_color(),
            content_rect.width(),
            1.0,
        );
        let text_rect = textbox_text_interactive_rect_from_galley(
            ui,
            content_rect,
            &layout.galley,
            default_text_size(),
            1.0,
            false,
        );

        assert!(text_rect.width() < content_rect.width());
        assert!(text_rect.height() <= content_rect.height());
        assert!(text_rect.contains(content_rect.min));
    });
}

#[test]
fn textbox_body_press_on_unselected_textbox_starts_pending_move() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            15,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "short".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));

        let annotation = state.annotation_by_id(15).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert_eq!(state.interaction.selected_annotation_ids, vec![15]);
        assert!(matches!(
            state.interaction.gesture,
            Some(crate::canvas::ActiveGesture::TextboxTransform(crate::canvas::TextboxTransformState::PendingMove { ref annotation_ids, .. }))
                if annotation_ids == &vec![15]
        ));
    });
}

#[test]
fn textbox_edge_press_on_unselected_textbox_selects_without_resizing() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            18,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "short".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.interaction.selected_annotation_ids.clear();

        let annotation = state.annotation_by_id(18).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert_eq!(state.interaction.selected_annotation_ids, vec![18]);
        assert!(matches!(
            state.interaction.gesture,
            Some(crate::canvas::ActiveGesture::TextboxTransform(crate::canvas::TextboxTransformState::PendingMove { ref annotation_ids, .. }))
                if annotation_ids == &vec![18]
        ));
    });
}

#[test]
fn textbox_body_press_while_editing_empty_exits_edit_and_starts_pending_move() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            16,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            String::new(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(16);

        let annotation = state.annotation_by_id(16).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert_eq!(state.interaction.editing_text_annotation_id, None);
        assert!(state.annotation_by_id(16).is_some());
        assert!(matches!(
            state.interaction.gesture,
            Some(crate::canvas::ActiveGesture::TextboxTransform(crate::canvas::TextboxTransformState::PendingMove { ref annotation_ids, .. }))
                if annotation_ids == &vec![16]
        ));
    });
}

#[test]
fn textbox_body_press_on_selected_multi_textbox_keeps_group_move() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        for (id, start_x) in [(21, 20.0), (22, 360.0)] {
            state.add_annotation(CanvasAnnotation::new(
                id,
                CanvasAnnotationKind::Textbox,
                eframe::egui::pos2(start_x, 40.0),
                eframe::egui::pos2(start_x + 220.0, 140.0),
                "group".to_owned(),
                default_text_size(),
                default_stroke_width(),
                default_annotation_color(),
                false,
                false,
            ));
        }
        state.interaction.selected_annotation_ids = vec![21, 22];

        let annotation = state.annotation_by_id(21).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert_eq!(state.interaction.selected_annotation_ids, vec![21, 22]);
        assert!(matches!(
            state.interaction.gesture,
            Some(crate::canvas::ActiveGesture::TextboxTransform(crate::canvas::TextboxTransformState::PendingMove { ref annotation_ids, .. }))
                if annotation_ids == &vec![21, 22]
        ));
    });
}

#[test]
fn textbox_text_press_while_editing_stays_editor_owned() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            17,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "hello world".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.begin_textbox_edit(17);

        let annotation = state.annotation_by_id(17).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let content_rect = textbox_content_rect(textbox_rect);
        let layout = state.cached_textbox_layout(
            ui,
            annotation.id,
            &annotation.text,
            default_text_size(),
            default_annotation_color(),
            content_rect.width(),
            1.0,
        );
        let text_rect = textbox_text_interactive_rect_from_galley(
            ui,
            content_rect,
            &layout.galley,
            default_text_size(),
            1.0,
            false,
        );
        let pointer_pos = text_rect.center();

        assert_eq!(
            textbox_pointer_region(ui, canvas_rect, &mut state, &annotation, pointer_pos),
            Some(TextboxPointerRegion::Text)
        );

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert_eq!(state.interaction.editing_text_annotation_id, Some(17));
        assert!(state.interaction.gesture.is_none());
    });
}

#[test]
fn begin_textbox_edit_selects_textbox_without_placeholder_content() {
    let mut state = CanvasState::default();
    state.add_annotation(CanvasAnnotation::new(
        5,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::pos2(260.0, 96.0),
        String::new(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));

    state.begin_textbox_edit(5);

    assert_eq!(state.interaction.editing_text_annotation_id, Some(5));
    assert_eq!(state.interaction.selected_annotation_ids, vec![5]);
    assert!(
        state
            .annotation_by_id(5)
            .is_some_and(|annotation| annotation.text.is_empty())
    );
}

#[test]
fn finishing_empty_textbox_edit_removes_and_archives_annotation() {
    let mut state = CanvasState::default();
    state.add_annotation(CanvasAnnotation::new(
        7,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::pos2(260.0, 80.0),
        String::new(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));

    state.begin_textbox_edit(7);
    state.finish_textbox_edit();

    assert!(state.document.annotations.is_empty());
    assert_eq!(
        state.take_document_changes().archived_annotation_ids,
        vec![7]
    );
}

#[test]
fn add_annotation_invalidates_textbox_layout_cache() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        state.cache_textbox_layout(
            5,
            "cached",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
            layout_textbox_galley(
                ui,
                "cached",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            ),
        );

        state.add_annotation(CanvasAnnotation::new(
            5,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
            String::new(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));

        assert!(!state.render.textbox_layout_cache.contains_key(&5));
    });
}

#[test]
fn remove_annotation_locally_invalidates_textbox_layout_cache() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation::new(
            3,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
            "hello".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.cache_textbox_layout(
            3,
            "hello",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
            layout_textbox_galley(
                ui,
                "hello",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            ),
        );

        state.remove_annotation_locally(3);

        assert!(!state.render.textbox_layout_cache.contains_key(&3));
    });
}

#[test]
fn set_textbox_world_rect_invalidates_textbox_layout_cache() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation::new(
            4,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
            "resize".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
        state.cache_textbox_layout(
            4,
            "resize",
            default_text_size(),
            default_annotation_color(),
            120.0,
            1.0,
            layout_textbox_galley(
                ui,
                "resize",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            ),
        );

        assert!(state.set_textbox_world_rect(
            4,
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(320.0, 120.0),
            ),
        ));
        assert!(!state.render.textbox_layout_cache.contains_key(&4));
    });
}

#[test]
fn annotation_paint_stats_cull_offscreen_textboxes() {
    let rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(0.0, 0.0),
        eframe::egui::pos2(800.0, 600.0),
    );
    let annotations = vec![
        CanvasAnnotation::new(
            30,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "visible".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ),
        CanvasAnnotation::new(
            31,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(2000.0, 40.0),
            eframe::egui::pos2(2300.0, 180.0),
            "offscreen-a".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ),
        CanvasAnnotation::new(
            32,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(2400.0, 260.0),
            eframe::egui::pos2(2700.0, 400.0),
            "offscreen-b".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ),
        CanvasAnnotation::new(
            33,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(2800.0, -120.0),
            eframe::egui::pos2(3100.0, 20.0),
            "offscreen-c".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ),
    ];

    let legacy = annotation_paint_stats_for_annotations(
        &annotations,
        rect,
        eframe::egui::Vec2::ZERO,
        8.0,
        false,
    );
    let current = annotation_paint_stats_for_annotations(
        &annotations,
        rect,
        eframe::egui::Vec2::ZERO,
        8.0,
        true,
    );

    assert_eq!(legacy.considered, 4);
    assert_eq!(legacy.drawn, 4);
    assert_eq!(current.considered, 4);
    assert_eq!(current.drawn, 1);
}

#[test]
fn context_click_on_unselected_annotation_selects_it_before_requesting_menu() {
    let mut state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(400.0, 300.0),
    );
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 40, 40, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(80.0, 0.0),
        eframe::egui::pos2(160.0, 40.0),
        "note".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));
    state.select_placements(&[1]);

    open_context_menu_at_pointer(
        &mut state,
        rect,
        rect.center() + eframe::egui::vec2(120.0, 20.0),
    );

    assert!(state.selected_placement_ids().is_empty());
    assert_eq!(state.selected_annotation_ids(), vec![20]);
    assert!(state.take_context_menu_request().is_some());
}
