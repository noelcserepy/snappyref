use eframe::egui;

pub(super) fn install(_cc: &eframe::CreationContext<'_>) {}

pub(super) fn hovered() -> super::NativeDropPayload {
    super::NativeDropPayload::default()
}

pub(super) fn cursor_screen_pos() -> Option<egui::Pos2> {
    None
}

pub(super) fn take_drop() -> super::NativeDropPayload {
    super::NativeDropPayload::default()
}

pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
    Vec::new()
}

pub(super) fn clipboard_pureref_binary() -> Option<Vec<u8>> {
    None
}
