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

pub(crate) fn import_source_label(source: &ExternalImportSource) -> String {
    match source {
        ExternalImportSource::Path(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image")
            .to_owned(),
        ExternalImportSource::Url(url) => {
            let parsed = url::Url::parse(url).ok();
            parsed
                .as_ref()
                .and_then(|parsed| {
                    parsed
                        .path_segments()
                        .and_then(Iterator::last)
                        .filter(|segment| !segment.is_empty())
                        .map(str::to_owned)
                })
                .or_else(|| {
                    parsed
                        .as_ref()
                        .and_then(|parsed| parsed.host_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "image-url".to_owned())
        }
    }
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
    use super::{ExternalImportSource, import_preview_size_world, import_source_label};

    #[test]
    fn import_preview_size_respects_import_area() {
        let size = import_preview_size_world(4000, 3000, 10_000.0);
        assert!(size.x * size.y <= 10_000.0 + 1.0);
    }

    #[test]
    fn import_source_label_prefers_url_path_basename_then_host() {
        assert_eq!(
            import_source_label(&ExternalImportSource::Url(
                "https://example.com/images/cat.png?size=large".to_owned()
            )),
            "cat.png"
        );
        assert_eq!(
            import_source_label(&ExternalImportSource::Url(
                "https://example.com/".to_owned()
            )),
            "example.com"
        );
    }
}
