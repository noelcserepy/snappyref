mod board_lifecycle;
mod completion;
mod control;
mod editing_interaction;
mod inspect_media;
mod inspect_scenarios;
mod performance_reporting;
mod placement_creation;
mod semantic_search;
mod support;
use support::{
    InspectImportSample, InspectProgressConfig, InspectScenario, add_inspect_compact_image,
    add_loaded_record, app_full_harness, app_full_harness_with_startup,
    app_full_harness_with_step_dt, app_harness, compact_selection_fact, drag_test_pos,
    dropped_file, inspect_bright_red_pixels_in_rect, inspect_click_at, inspect_command_modifier,
    inspect_placement, inspect_pos, inspect_prepared_loaded_placement, inspect_rect,
    inspect_search_result_entry, inspect_search_result_tile_center, inspect_status_slug,
    inspect_wait_for_search_previews, inspect_world_to_screen, key_press_input, loaded_record,
    persisted_loaded_item, render_sidebar_for_test, solid_rgba, test_app, test_canvas_annotation,
    test_loaded_item, test_screen_rect, test_viewport_world,
};

use super::perf_scenario::{EmbeddingPhaseMarkers, ModelLoadTimingSummary};
use super::{AppConfig, ArrangementAction, SnappyRefApp, apply_arrangement_action};
use crate::data::{
    Asset, CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, Placement, PlacementState,
};
use crate::image_preparation::PreparedPlacementImage;
use crate::library::storage::Library;
use crate::search::palette::PALETTE_WIDTH;
use crate::search::semantic::{LocalModelStoreStatus, SearchResultAssetEntry};
use crate::test_support::TempRoot;
use crate::ui::sidebar::{SidebarTab, block_sidebar_input, sidebar_rect};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

mod groups;
mod inspect_operations;
mod inspect_pureref;
mod session_settings;

#[derive(Clone, Copy, PartialEq)]
enum StackContentKind {
    Placement,
    Annotation,
}

/// Seeds four items in stack order (back, upper, lower, front), selects the
/// two middle ones, runs `copy_selection`, and asserts the copies land at the
/// front of the stack in the selection's original relative order, both in
/// memory and in the library.
fn assert_copy_keeps_stack_order(
    kinds: [StackContentKind; 4],
    copy_selection: impl FnOnce(&mut SnappyRefApp),
) {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-copy-stack-order");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.placement_gap = 24.0;
    let mut asset_id = None;
    let mut stack = Vec::new();
    for (index, kind) in kinds.into_iter().enumerate() {
        let position = egui::pos2(index as f32 * 30.0, index as f32 * 20.0);
        let item = match kind {
            StackContentKind::Placement => {
                let placement = match asset_id {
                    None => library
                        .fixture_placement(
                            library.load_user_session().unwrap().active_board_id,
                            crate::data::Placement::loaded(
                                crate::data::PlacementState::new(
                                    library.allocate_document_id(),
                                    position,
                                    1.0,
                                    0.0,
                                    0,
                                    false,
                                ),
                                library
                                    .register_original(&image_path, "image/png", 1, 1, 4, None)
                                    .unwrap(),
                            ),
                        )
                        .unwrap(),
                    Some(asset_id) => library
                        .fixture_placement(
                            library.load_user_session().unwrap().active_board_id,
                            library
                                .new_placement_from_asset(asset_id, position, 1.0, 0.0, 0)
                                .unwrap(),
                        )
                        .unwrap(),
                };
                asset_id = Some(placement.require_loaded_asset().id);
                add_loaded_record(&mut app, &ctx, placement.clone());
                CanvasStackItem::Placement(placement.state.id)
            }
            StackContentKind::Annotation => {
                let fixture = test_canvas_annotation(0, position);
                let annotation = library
                    .fixture_annotation(
                        fixture.kind,
                        fixture.start(),
                        fixture.end(),
                        &fixture.text,
                        fixture.text_size,
                        fixture.stroke_width,
                        fixture.color,
                    )
                    .unwrap();
                app.canvas.add_annotation(annotation.clone());
                CanvasStackItem::Annotation(annotation.id)
            }
        };
        stack.push(item);
    }
    app.canvas.set_canvas_stack_order(stack.clone());
    let selected = [stack[2], stack[1]];
    let ids_of = |kind: StackContentKind| -> Vec<i64> {
        selected
            .iter()
            .filter_map(|item| match (kind, item) {
                (StackContentKind::Placement, CanvasStackItem::Placement(id))
                | (StackContentKind::Annotation, CanvasStackItem::Annotation(id)) => Some(*id),
                _ => None,
            })
            .collect()
    };
    app.canvas.select_content(
        &ids_of(StackContentKind::Placement),
        &ids_of(StackContentKind::Annotation),
    );

    copy_selection(&mut app);

    // Copies of one kind are selected in the same order as their originals.
    let mut copied_placements = app.canvas.selected_placement_ids().into_iter();
    let mut copied_annotations = app.canvas.selected_annotation_ids().into_iter();
    let mut copy_of = |item: &CanvasStackItem| match item {
        CanvasStackItem::Placement(_) => {
            CanvasStackItem::Placement(copied_placements.next().unwrap())
        }
        CanvasStackItem::Annotation(_) => {
            CanvasStackItem::Annotation(copied_annotations.next().unwrap())
        }
    };
    let lower_copy = copy_of(&stack[2]);
    let upper_copy = copy_of(&stack[1]);
    assert!(copied_placements.next().is_none() && copied_annotations.next().is_none());
    let mut expected = stack;
    expected.extend([upper_copy, lower_copy]);
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    app.flush_persistence();
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);
}

fn paste_selection(app: &mut SnappyRefApp) {
    let payload = app.copy_selected_board_payload().unwrap();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();
}
