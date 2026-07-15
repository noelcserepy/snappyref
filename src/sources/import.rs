use std::path::PathBuf;

use eframe::egui;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExternalImportSource {
    Path(PathBuf),
    Url(String),
}

pub(crate) fn normalize_import_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    ((parsed.scheme() == "http") || (parsed.scheme() == "https")).then(|| parsed.to_string())
}

pub(crate) fn import_preview_size_world(
    width: u32,
    height: u32,
    placement_target_area: f32,
) -> egui::Vec2 {
    let scale = crate::jobs::display_scale_for_placement_target_area(
        width,
        height,
        true,
        placement_target_area,
    );
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

#[cfg(test)]
mod tests {
    use super::import_preview_size_world;

    #[test]
    fn import_preview_size_respects_import_area() {
        let size = import_preview_size_world(4000, 3000, 10_000.0);
        assert!(size.x * size.y <= 10_000.0 + 1.0);
    }
}
