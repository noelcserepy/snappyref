use crate::active_board::editing::BoardEditing;
use crate::canvas::{CanvasInputPolicy, CanvasState};
use crate::data::{Asset, CanvasImageLod, Placement, PlacementState, WorkspaceSettings};
use crate::image_preparation::{
    ImagePrepPreviewTexture, ImagePrepResult, ImportOrigin, PreparedPlacementImage,
};
use crate::placement::completion::{CompletionContext, PreparationOutcome};
use crate::placement::content_resolution::PlacementCreation;
use eframe::egui;

fn preview(asset_id: i64, lod: CanvasImageLod) -> PreparedPlacementImage {
    PreparedPlacementImage {
        target_board_id: 1,
        queue_id: None,
        placement: Placement::loaded(
            PlacementState::new(7, egui::Pos2::ZERO, 1.0, 0.0, 0, false),
            Asset {
                id: asset_id,
                original_path: "image.png".into(),
                filename: "image.png".to_owned(),
                mime_type: "image/png".to_owned(),
                width: 256,
                height: 256,
            },
        ),
        preview_rgba: [30, 100, 220, 255].repeat(16),
        preview_width: 4,
        preview_height: 4,
        preview_texture: ImagePrepPreviewTexture::Cpu,
        loaded_lod: lod,
        animated_preview: None,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Existing,
        trace: None,
    }
}

fn paint(ctx: &egui::Context, state: &mut CanvasState) -> Vec<egui::TextureId> {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let output = crate::ui::test_frame::run_ui(
        ctx,
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ui| {
            let frame = crate::canvas::show(
                ui,
                rect,
                rect,
                state,
                None,
                true,
                CanvasInputPolicy::Blocked,
                false,
            );
            assert_eq!(frame.image_pass.painted_images, 1);
        },
    );
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                Some(mesh.texture_id)
            }
            _ => None,
        })
        .collect()
}

fn fail_load(ctx: &egui::Context, canvas: &mut CanvasState, asset_id: i64, lod: CanvasImageLod) {
    let mut creation = PlacementCreation::new();
    let mut editing = BoardEditing::default();
    let mut residency = crate::canvas::residency::ImageResidencyCoordinator::default();
    let settings = WorkspaceSettings::default();
    let outcome = creation.accept_result(
        ctx,
        &mut CompletionContext {
            canvas,
            library: None,
            board_id: 1,
            worker: None,
            settings: &settings,
            residency: &mut residency,
            editing: &mut editing,
        },
        ImagePrepResult::ExistingLoadFailed {
            asset_id: Some(asset_id),
            target_board_id: 1,
            queue_id: None,
            placement_id: 7,
            lod,
            context: "image.png".to_owned(),
            error: "fixture decode failure".to_owned(),
        },
    );
    match outcome {
        PreparationOutcome::Status(Some(status)) => {
            assert_eq!(status, "image.png: fixture decode failure");
        }
        _ => panic!("expected load error status"),
    }
}

#[test]
fn failed_lod_is_requested_once_and_keeps_its_preview_until_reset() {
    for (loaded_lod, failed_lod) in [
        (CanvasImageLod::Overview, CanvasImageLod::Working),
        (CanvasImageLod::Working, CanvasImageLod::Detail),
    ] {
        for reset in ["success", "reload", "content change"] {
            let ctx = egui::Context::default();
            let mut canvas = CanvasState::default();
            canvas.add_prepared_placement_image(&ctx, preview(7, loaded_lod));
            let texture = canvas.rendered_placement_by_id(7).unwrap().texture.id();
            assert_eq!(paint(&ctx, &mut canvas), vec![texture]);
            assert_eq!(canvas.undispatched_image_lod_loads(), vec![(7, failed_lod)]);

            fail_load(&ctx, &mut canvas, 7, failed_lod);
            for _ in 0..4 {
                assert_eq!(paint(&ctx, &mut canvas), vec![texture]);
                assert_eq!(canvas.undispatched_image_lod_loads(), vec![]);
                assert_eq!(canvas.placement_for_id(7).unwrap().asset_id(), Some(7));
                assert!(!canvas.has_pending_image_loads());
            }

            if reset == "reload" {
                canvas.clear_for_board_switch();
            }
            let asset_id = if reset == "content change" { 8 } else { 7 };
            canvas.add_prepared_placement_image(&ctx, preview(asset_id, loaded_lod));
            let next_texture = canvas.rendered_placement_by_id(7).unwrap().texture.id();
            assert_eq!(paint(&ctx, &mut canvas), vec![next_texture]);
            assert_eq!(canvas.undispatched_image_lod_loads(), vec![(7, failed_lod)]);
            assert_eq!(
                canvas.placement_for_id(7).unwrap().asset_id(),
                Some(asset_id)
            );
        }
    }
}

#[test]
fn late_load_failure_keeps_other_lod_and_replacement_requests() {
    let ctx = egui::Context::default();
    let mut canvas = CanvasState::default();
    canvas.add_prepared_placement_image(&ctx, preview(7, CanvasImageLod::Overview));
    paint(&ctx, &mut canvas);
    assert_eq!(
        canvas.undispatched_image_lod_loads(),
        vec![(7, CanvasImageLod::Working)]
    );
    canvas.note_image_lod_load_dispatched(7, CanvasImageLod::Detail);
    fail_load(&ctx, &mut canvas, 7, CanvasImageLod::Working);
    assert!(canvas.has_pending_image_loads());

    canvas.add_prepared_placement_image(&ctx, preview(8, CanvasImageLod::Overview));
    let texture = canvas.rendered_placement_by_id(7).unwrap().texture.id();
    assert_eq!(paint(&ctx, &mut canvas), vec![texture]);
    assert_eq!(
        canvas.undispatched_image_lod_loads(),
        vec![(7, CanvasImageLod::Working)]
    );
    fail_load(&ctx, &mut canvas, 7, CanvasImageLod::Working);
    assert!(canvas.has_pending_image_loads());
    assert_eq!(paint(&ctx, &mut canvas), vec![texture]);
    assert_eq!(canvas.placement_for_id(7).unwrap().asset_id(), Some(8));
}
