use eframe::egui;

/// Right-aligned row layout for trailing action groups.
pub(crate) fn row_end_center_y_layout() -> egui::Layout {
    egui::Layout::right_to_left(egui::Align::Center)
}

/// Left-to-right centered row layout for nested layout overrides.
pub(crate) fn row_center_y_layout() -> egui::Layout {
    egui::Layout::left_to_right(egui::Align::Center)
}

/// Top-left position for manually painted content centered inside a rect.
pub(crate) fn centered_pos(rect: egui::Rect, size: egui::Vec2) -> egui::Pos2 {
    rect.center() - size * 0.5
}
