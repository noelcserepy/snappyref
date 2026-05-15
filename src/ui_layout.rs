use eframe::egui;

/// Right-aligned row layout for trailing action groups.
pub(crate) fn row_end_center_y_layout() -> egui::Layout {
    egui::Layout::right_to_left(egui::Align::Center)
}

/// Left-to-right centered row layout for nested layout overrides.
pub(crate) fn row_center_y_layout() -> egui::Layout {
    egui::Layout::left_to_right(egui::Align::Center)
}

/// Default stacked content layout.
#[allow(dead_code)]
pub(crate) fn column_left() -> egui::Layout {
    egui::Layout::top_down(egui::Align::Min)
}

/// Rare stacked layout for brief status blocks and spatial previews.
#[allow(dead_code)]
pub(crate) fn column_center_x() -> egui::Layout {
    egui::Layout::top_down(egui::Align::Center)
}

/// Full-rect centering for one child only.
#[allow(dead_code)]
pub(crate) fn centered_single() -> egui::Layout {
    egui::Layout::centered_and_justified(egui::Direction::TopDown)
}

/// Top-left position for manually painted content centered inside a rect.
pub(crate) fn centered_pos(rect: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    rect.center() - size * 0.5
}
