use super::*;

#[test]
fn textbox_draft_keeps_dragged_rect_and_enforces_minimum_note_size() {
    let start = eframe::egui::pos2(20.0, 40.0);
    // (pointer release point, expected end corner); the minimum note is 240x56.
    let cases = [
        (start, eframe::egui::pos2(260.0, 96.0)),
        (
            eframe::egui::pos2(32.0, 56.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        (
            eframe::egui::pos2(340.0, 120.0),
            eframe::egui::pos2(340.0, 120.0),
        ),
    ];

    for (release, expected_end) in cases {
        let mut draft = AnnotationDraftState::new(CanvasAnnotationKind::Textbox, start);
        draft.set_current_world(release);
        let pending = pending_annotation_from_draft(draft).unwrap();

        assert_eq!(pending.start_world(), start, "release at {release:?}");
        assert_eq!(pending.end_world(), expected_end, "release at {release:?}");
    }
}

#[test]
fn tall_textbox_content_grows_past_minimum_height() {
    assert!(textbox_required_world_height(80.0, 1.0) > 56.0);
}
#[test]
fn textbox_content_rect_stays_inside_zoomed_out_textbox() {
    let state = CanvasState {
        zoom: MIN_CANVAS_ZOOM,
        ..Default::default()
    };
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
fn textbox_toolbar_sits_above_textbox_at_its_left_edge_when_space_allows() {
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
    let toolbar_rect = editing_textbox_toolbar_rect(&state, canvas_rect).unwrap();

    assert!((toolbar_rect.left() - textbox_rect.left()).abs() < 0.1);
    assert!(
        (textbox_rect.top() - toolbar_rect.bottom() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE).abs() < 0.1
    );
}

#[test]
fn textbox_toolbar_clamps_to_top_margin_when_above_gap_does_not_fit() {
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

    let toolbar_rect = editing_textbox_toolbar_rect(&state, canvas_rect).unwrap();

    assert!(
        (toolbar_rect.top() - (canvas_rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN)).abs()
            < 0.1
    );
}

#[test]
fn textbox_toolbar_clamps_right_without_unnecessary_shift() {
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

    let toolbar_rect = editing_textbox_toolbar_rect(&state, canvas_rect).unwrap();

    assert!(
        toolbar_rect.right() <= canvas_rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN + 0.1
    );
    assert!(
        toolbar_rect.left() >= canvas_rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - 0.1
    );
}

#[test]
fn textbox_layout_cache_reuses_galley_only_for_identical_inputs() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        // egui dedupes identical layouts, so seed a galley wrapped at another width.
        // Only a textbox cache hit can return it for a 120-wide request.
        let seeded = layout_textbox_galley(
            ui,
            "note",
            default_text_size(),
            default_annotation_color(),
            300.0,
            1.0,
        );
        let seed = |state: &mut CanvasState| {
            state.cache_textbox_layout(
                1,
                "note",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                seeded.clone(),
            );
        };
        let layout = |state: &mut CanvasState, text: &str, zoom: f32| {
            state
                .cached_textbox_layout(
                    ui,
                    1,
                    text,
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    zoom,
                )
                .galley
        };

        seed(&mut state);
        assert!(Arc::ptr_eq(&seeded, &layout(&mut state, "note", 1.0)));
        seed(&mut state);
        assert!(!Arc::ptr_eq(&seeded, &layout(&mut state, "note", 2.0)));
        seed(&mut state);
        assert!(!Arc::ptr_eq(
            &seeded,
            &layout(&mut state, "edited note", 1.0)
        ));
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
fn offscreen_textboxes_are_culled_from_painting() {
    let state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(0.0, 0.0),
        eframe::egui::pos2(800.0, 600.0),
    );
    let cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    let textbox = |id, min: eframe::egui::Pos2| {
        CanvasAnnotation::new(
            id,
            CanvasAnnotationKind::Textbox,
            min,
            min + eframe::egui::vec2(300.0, 140.0),
            "note".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        )
    };

    assert!(annotation_should_paint(
        &state,
        rect,
        cull_rect,
        &textbox(30, eframe::egui::pos2(20.0, 40.0))
    ));
    assert!(!annotation_should_paint(
        &state,
        rect,
        cull_rect,
        &textbox(31, eframe::egui::pos2(2000.0, 40.0))
    ));
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
