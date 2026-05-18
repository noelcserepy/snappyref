use eframe::egui;

use super::super::{CanvasState, CropState};

pub(crate) fn begin_crop(
    state: &mut CanvasState,
    placement_id: i64,
    pointer_pos: egui::Pos2,
) -> bool {
    if !state.can_start_crop_on_placement(placement_id) {
        return false;
    }
    state.set_single_selection(placement_id);
    state.interaction.drag_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.marquee_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.crop_state = Some(CropState {
        placement_id,
        start_screen: pointer_pos,
        current_screen: pointer_pos,
    });
    true
}

pub(crate) fn update_crop(state: &mut CanvasState, pointer_pos: egui::Pos2) {
    if let Some(crop_state) = &mut state.interaction.crop_state {
        crop_state.current_screen = pointer_pos;
    }
}

pub(crate) fn finish_crop(state: &mut CanvasState, rect: egui::Rect) -> bool {
    let Some(crop_state) = state.interaction.crop_state.take() else {
        return false;
    };
    state.interaction.pending_crop = state.build_crop_request(rect, &crop_state);
    true
}
