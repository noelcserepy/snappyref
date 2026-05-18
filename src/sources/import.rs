use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ExternalImportSource {
    Path(PathBuf),
    Url(String),
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasImportPlaceholder {
    pub(crate) queue_id: u64,
    pub(crate) label: String,
    pub(crate) center_world: egui::Pos2,
    pub(crate) size_world: egui::Vec2,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImportReservation {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
    pub(crate) reserved_world: Option<egui::Pos2>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImportPlacementSettings {
    pub(crate) import_target_canvas_area: f32,
}

#[derive(Default)]
pub(crate) struct ImportIntake {
    next_queue_id: u64,
    placeholders_by_board: HashMap<i64, Vec<CanvasImportPlaceholder>>,
}

impl ImportIntake {
    pub(crate) fn new() -> Self {
        Self {
            next_queue_id: 1,
            placeholders_by_board: HashMap::new(),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reserve_import(
        &mut self,
        layout: &mut Vec<crate::data::PlacementLayoutItem>,
        target_board_id: i64,
        label: String,
        dimensions: Option<(u32, u32)>,
        desired_world: egui::Pos2,
        settings: ImportPlacementSettings,
    ) -> (ImportReservation, Option<CanvasImportPlaceholder>) {
        self.reserve_import_at_world(
            layout,
            target_board_id,
            label,
            dimensions,
            desired_world,
            settings,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn reserve_import_at_world(
        &mut self,
        layout: &mut Vec<crate::data::PlacementLayoutItem>,
        target_board_id: i64,
        label: String,
        dimensions: Option<(u32, u32)>,
        center_world: egui::Pos2,
        settings: ImportPlacementSettings,
    ) -> (ImportReservation, Option<CanvasImportPlaceholder>) {
        let queue_id = self.next_queue_id();
        let placeholder = dimensions.map(|(width, height)| {
            let size_world =
                import_preview_size_world(width, height, settings.import_target_canvas_area);
            layout.push(crate::data::PlacementLayoutItem {
                id: -((layout.len() as i64) + 1),
                center: center_world,
                width: size_world.x,
                height: size_world.y,
            });
            let placeholder = CanvasImportPlaceholder {
                queue_id,
                label,
                center_world,
                size_world,
            };
            self.queue_placeholder(target_board_id, placeholder.clone());
            placeholder
        });

        (
            ImportReservation {
                target_board_id,
                queue_id,
                reserved_world: placeholder
                    .as_ref()
                    .map(|placeholder| placeholder.center_world),
            },
            placeholder,
        )
    }

    pub(crate) fn remove_placeholder_for_board(&mut self, board_id: i64, queue_id: u64) {
        if let Some(placeholders) = self.placeholders_by_board.get_mut(&board_id) {
            placeholders.retain(|placeholder| placeholder.queue_id != queue_id);
        }
    }

    pub(crate) fn placeholders_for_board(&self, board_id: i64) -> Vec<CanvasImportPlaceholder> {
        self.placeholders_by_board
            .get(&board_id)
            .cloned()
            .unwrap_or_default()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn queue_placeholder_for_board(
        &mut self,
        board_id: i64,
        queue_id: u64,
        label: String,
        center_world: egui::Pos2,
        width: u32,
        height: u32,
        auto_image_scaling: bool,
        import_target_canvas_area: f32,
    ) -> CanvasImportPlaceholder {
        let placeholder = CanvasImportPlaceholder {
            queue_id,
            label,
            center_world,
            size_world: import_size_world(
                width,
                height,
                auto_image_scaling,
                import_target_canvas_area,
            ),
        };
        self.queue_placeholder(board_id, placeholder.clone());
        placeholder
    }

    fn next_queue_id(&mut self) -> u64 {
        if self.next_queue_id == 0 {
            self.next_queue_id = 1;
        }
        let queue_id = self.next_queue_id;
        self.next_queue_id = self.next_queue_id.saturating_add(1);
        queue_id
    }

    fn queue_placeholder(&mut self, board_id: i64, placeholder: CanvasImportPlaceholder) {
        let placeholders = self.placeholders_by_board.entry(board_id).or_default();
        placeholders.retain(|existing| existing.queue_id != placeholder.queue_id);
        placeholders.push(placeholder);
    }
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

fn import_size_world(
    width: u32,
    height: u32,
    auto_image_scaling: bool,
    target_canvas_area: f32,
) -> egui::Vec2 {
    let scale =
        crate::jobs::initial_import_scale(width, height, auto_image_scaling, target_canvas_area);
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

pub(crate) fn import_preview_size_world(
    width: u32,
    height: u32,
    target_canvas_area: f32,
) -> egui::Vec2 {
    import_size_world(width, height, true, target_canvas_area)
}

#[cfg(test)]
mod tests {
    use super::{ExternalImportSource, ImportIntake, ImportPlacementSettings, import_source_label};

    #[test]
    fn reserve_import_tracks_board_scoped_placeholder() {
        let mut intake = ImportIntake::new();
        let mut layout = Vec::new();

        let (reservation, placeholder) = intake.reserve_import(
            &mut layout,
            2,
            "Alpha".to_owned(),
            Some((100, 50)),
            eframe::egui::pos2(10.0, 20.0),
            ImportPlacementSettings {
                import_target_canvas_area: crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            },
        );

        assert_eq!(reservation.target_board_id, 2);
        assert_eq!(reservation.queue_id, 1);
        assert_eq!(
            reservation.reserved_world,
            Some(eframe::egui::pos2(10.0, 20.0))
        );
        assert!(placeholder.is_some());
        assert_eq!(intake.placeholders_for_board(1).len(), 0);
        assert_eq!(intake.placeholders_for_board(2).len(), 1);
    }

    #[test]
    fn drop_preview_placeholders_respect_import_area_when_auto_scaling_is_off() {
        let mut intake = ImportIntake::new();
        let mut layout = Vec::new();

        let (_reservation, placeholder) = intake.reserve_import(
            &mut layout,
            1,
            "large.png".to_owned(),
            Some((4000, 3000)),
            eframe::egui::Pos2::ZERO,
            ImportPlacementSettings {
                import_target_canvas_area: 10_000.0,
            },
        );

        let placeholder = placeholder.expect("known image dimensions should create a preview");
        assert!(placeholder.size_world.x * placeholder.size_world.y <= 10_000.0 + 1.0);
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
