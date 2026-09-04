use eframe::egui;

use super::super::{ActiveGesture, CanvasState, CropState};

pub(crate) fn begin_crop(
    state: &mut CanvasState,
    placement_id: i64,
    pointer_pos: egui::Pos2,
) -> bool {
    if !state.can_start_crop_on_placement(placement_id) {
        return false;
    }
    state.set_single_selection(placement_id);
    state.interaction.gesture = Some(ActiveGesture::Crop(CropState {
        placement_id,
        start_screen: pointer_pos,
        current_screen: pointer_pos,
    }));
    true
}
