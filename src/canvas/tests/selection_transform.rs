use super::*;

#[test]
fn focus_selection_in_view_fits_selected_bounds_inside_target_rect() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(960.0, 640.0),
        );
        let target_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(960.0, 420.0),
        );
        assert!(!state.focus_selection_in_view(canvas_rect, target_rect));

        push_test_image(
            ui,
            &mut state,
            1,
            300,
            180,
            1.0,
            eframe::egui::pos2(-300.0, -120.0),
        );
        push_test_image(
            ui,
            &mut state,
            2,
            180,
            320,
            1.0,
            eframe::egui::pos2(260.0, 180.0),
        );
        state.select_placements(&[1, 2]);

        assert!(state.focus_selection_in_view(canvas_rect, target_rect));

        let selection_rect = state.selection_screen_rect(canvas_rect).unwrap();
        assert!(
            target_rect
                .shrink(FOCUS_SELECTION_PADDING_SCREEN)
                .contains_rect(selection_rect)
        );
    });
}

#[test]
fn textbox_resize_respects_minimum_size() {
    let resized = resized_textbox_rect(
        TextboxResizeHandle::BottomRight,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        eframe::egui::pos2(30.0, 50.0),
    );

    assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
    assert_eq!(resized.max, eframe::egui::pos2(36.0, 56.0));

    let ordinary = resized_textbox_rect(
        TextboxResizeHandle::BottomRight,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        eframe::egui::pos2(320.0, 120.0),
    );
    assert_eq!(ordinary.min, eframe::egui::pos2(20.0, 40.0));
    assert_eq!(ordinary.max, eframe::egui::pos2(320.0, 120.0));
}

#[test]
fn textbox_edge_resize_moves_one_side_only() {
    let resized = resized_textbox_rect(
        TextboxResizeHandle::Right,
        eframe::egui::pos2(20.0, 68.0),
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        eframe::egui::pos2(320.0, 20.0),
    );

    assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
    assert_eq!(resized.max, eframe::egui::pos2(320.0, 96.0));
}

#[test]
fn textbox_resize_hit_test_matches_visible_right_edge_zone() {
    let state = CanvasState::default();
    let canvas_rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(0.0, 0.0),
        eframe::egui::pos2(800.0, 600.0),
    );
    let annotation = CanvasAnnotation::new(
        1,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(20.0, 40.0),
        eframe::egui::pos2(260.0, 96.0),
        String::new(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
    );

    let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
    let pointer_pos = eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

    assert_eq!(
        hit_test_textbox_resize_handle(&state, canvas_rect, &annotation, pointer_pos),
        Some(TextboxResizeHandle::Right)
    );
}

#[test]
fn placement_resize_hit_test_matches_corner_zone() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        push_test_image(
            ui,
            &mut state,
            21,
            200,
            100,
            1.0,
            eframe::egui::pos2(200.0, 160.0),
        );
        let image = state.placement_by_id(21).unwrap();
        let image_rect = placement_screen_rect(&state, canvas_rect, image);
        let pointer_pos = image_rect.right_bottom() + eframe::egui::vec2(8.0, 8.0);

        assert_eq!(
            hit_test_placement_resize_handle(&state, canvas_rect, image, pointer_pos),
            Some(PlacementResizeHandle::BottomRight)
        );
    });
}

#[test]
fn placement_resize_scales_proportionally_from_opposite_corner() {
    let asset = Asset {
        id: 22,
        original_path: PathBuf::from("image-22.png"),
        filename: "image-22.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 200,
        height: 100,
    };
    let anchor = eframe::egui::pos2(200.0, 190.0);
    let start_handle = eframe::egui::pos2(400.0, 290.0);
    let pointer_pos = eframe::egui::pos2(600.0, 390.0);

    let (center, scale) =
        resized_placement_transform(&asset, 1.0, anchor, start_handle, pointer_pos);

    assert!((scale - 2.0).abs() < 0.01);
    assert_eq!(center, eframe::egui::pos2(400.0, 290.0));
}

#[test]
fn placement_resize_respects_minimum_scale() {
    let asset = Asset {
        id: 23,
        original_path: PathBuf::from("image-23.png"),
        filename: "image-23.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 400,
        height: 200,
    };

    let (center, scale) = resized_placement_transform(
        &asset,
        1.0,
        eframe::egui::pos2(100.0, 100.0),
        eframe::egui::pos2(500.0, 300.0),
        eframe::egui::pos2(100.5, 100.5),
    );

    assert_eq!(scale, 0.12);
    assert_eq!(center, eframe::egui::pos2(124.0, 112.0));
}

#[test]
fn rectangle_edge_press_on_selected_rectangle_starts_resize() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        state.add_annotation(CanvasAnnotation::new(
            19,
            CanvasAnnotationKind::Rectangle,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(320.0, 180.0),
            "short".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
        ));
        state.interaction.selected_annotation_ids = vec![19];

        let annotation = state.annotation_by_id(19).unwrap().clone();
        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

        assert!(handle_textbox_primary_press(
            ui,
            canvas_rect,
            &mut state,
            &annotation,
            pointer_pos,
        ));
        assert!(matches!(
            state.interaction.gesture,
            Some(crate::canvas::ActiveGesture::TextboxTransform(
                crate::canvas::TextboxTransformState::Resizing {
                    annotation_id: 19,
                    dragged_handle: TextboxResizeHandle::Right,
                    ..
                }
            ))
        ));
    });
}

#[test]
fn marquee_selects_textboxes_and_shift_marquee_adds_to_them() {
    let mut state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(960.0, 640.0),
    );
    for (id, min, max) in [
        (
            12,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(260.0, 96.0),
        ),
        (
            13,
            eframe::egui::pos2(280.0, 40.0),
            eframe::egui::pos2(420.0, 96.0),
        ),
    ] {
        state.add_annotation(CanvasAnnotation::new(
            id,
            CanvasAnnotationKind::Textbox,
            min,
            max,
            "textbox".to_owned(),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
        ));
    }
    let drag_marquee_over = |state: &mut CanvasState, id: i64, shift_held: bool| {
        let textbox = textbox_screen_rect(state, rect, state.annotation_by_id(id).unwrap());
        begin_marquee_selection(
            state,
            textbox.left_top() - eframe::egui::vec2(4.0, 4.0),
            shift_held,
        );
        update_marquee_selection(state, rect, textbox.center(), shift_held);
    };

    drag_marquee_over(&mut state, 12, false);
    assert!(state.interaction.selected_placement_ids.is_empty());
    assert_eq!(state.interaction.selected_annotation_ids, vec![12]);

    drag_marquee_over(&mut state, 13, true);
    assert_eq!(state.interaction.selected_annotation_ids, vec![12, 13]);
}
#[test]
fn compact_arrangement_respects_rotated_placement_display_bounds() {
    let mut state = CanvasState::default();
    let gap = 12.0;

    eframe::egui::__run_test_ui(|ui| {
        push_test_image(
            ui,
            &mut state,
            1,
            100,
            300,
            1.0,
            eframe::egui::pos2(0.0, 0.0),
        );
        push_test_image(
            ui,
            &mut state,
            2,
            180,
            100,
            1.0,
            eframe::egui::pos2(500.0, 0.0),
        );
    });
    assert!(state.set_placement_rotation(1, std::f32::consts::FRAC_PI_2));
    state.select_placements(&[1, 2]);

    assert_eq!(state.arrange_selected_compact(gap), 2);

    let first = state
        .document
        .placements
        .iter()
        .find(|image| image.state.id == 1)
        .unwrap();
    let second = state
        .document
        .placements
        .iter()
        .find(|image| image.state.id == 2)
        .unwrap();
    let first_rect = placement_visual_bounds_world(first);
    let second_rect = placement_visual_bounds_world(second);

    assert!(
        rectangles_respect_gap(first_rect, second_rect, gap),
        "rotated placement display bounds should not overlap arranged neighbor"
    );
}

#[test]
fn compact_arrangement_is_idempotent_for_same_selection() {
    let mut state = CanvasState::default();
    let gap = 12.0;

    eframe::egui::__run_test_ui(|ui| {
        push_test_image(
            ui,
            &mut state,
            1,
            180,
            120,
            1.0,
            eframe::egui::pos2(-300.0, 0.0),
        );
        push_test_image(
            ui,
            &mut state,
            2,
            90,
            180,
            1.0,
            eframe::egui::pos2(0.0, 200.0),
        );
        push_test_image(
            ui,
            &mut state,
            3,
            140,
            90,
            1.0,
            eframe::egui::pos2(240.0, -120.0),
        );
        push_test_image(
            ui,
            &mut state,
            4,
            80,
            80,
            1.0,
            eframe::egui::pos2(460.0, 80.0),
        );
    });
    state.select_placements(&[1, 2, 3, 4]);

    assert_eq!(state.arrange_selected_compact(gap), 4);
    let first_positions = selected_placement_positions(&state);
    assert_eq!(state.arrange_selected_compact(gap), 0);
    let second_positions = selected_placement_positions(&state);

    assert_eq!(first_positions.len(), second_positions.len());
    for ((first_id, first_pos), (second_id, second_pos)) in
        first_positions.iter().zip(second_positions.iter())
    {
        assert_eq!(first_id, second_id);
        assert!(
            (first_pos.x - second_pos.x).abs() < 0.001,
            "item {first_id} x moved from {} to {}",
            first_pos.x,
            second_pos.x
        );
        assert!(
            (first_pos.y - second_pos.y).abs() < 0.001,
            "item {first_id} y moved from {} to {}",
            first_pos.y,
            second_pos.y
        );
    }
}

#[test]
fn moved_canvas_stack_order_controls_overlapping_context_selection() {
    let mut state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(400.0, 300.0),
    );
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(-40.0, -40.0),
        eframe::egui::pos2(40.0, 40.0),
        "note".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
    ));
    state.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(20),
        CanvasStackItem::Placement(1),
    ]);
    state.select_content(&[], &[20]);

    assert!(state.bring_selection_to_front());
    open_context_menu_at_pointer(&mut state, rect, rect.center());

    assert!(state.selected_placement_ids().is_empty());
    assert_eq!(state.selected_annotation_ids(), vec![20]);
    assert!(state.take_context_menu_request().is_some());
}

#[test]
fn context_click_inside_existing_mixed_selection_preserves_selection() {
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
    state.select_content(&[1], &[20]);

    open_context_menu_at_pointer(&mut state, rect, rect.center());

    assert_eq!(state.selected_placement_ids(), vec![1]);
    assert_eq!(state.selected_annotation_ids(), vec![20]);
    assert!(state.take_context_menu_request().is_some());
}

#[test]
fn context_click_on_grouped_image_selects_group_textbox_members() {
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
    state.restore_groups(vec![crate::data::Group {
        id: 7,
        name: "References".into(),
        members: vec![1, 20],
        container: crate::data::RectGeometry::new(
            eframe::egui::pos2(60.0, 0.0),
            eframe::egui::vec2(240.0, 120.0),
            1.0,
            0.0,
            0,
        ),
        flow: crate::data::GroupFlow::Row,
        gap: 8.0,
        padding: 12.0,
        layout: Default::default(),
    }]);

    open_context_menu_at_pointer(&mut state, rect, rect.center());

    assert_eq!(state.selected_placement_ids(), vec![1]);
    assert_eq!(state.selected_annotation_ids(), vec![20]);
}
