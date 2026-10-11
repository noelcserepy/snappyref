use super::*;

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
fn rectangle_edge_press_on_unselected_rectangle_selects_without_resizing() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            18,
            CanvasAnnotationKind::Rectangle,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "short".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
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
