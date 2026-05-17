use std::collections::HashSet;

use eframe::egui;

use crate::import_intake::{ExternalImportSource, normalize_import_url};

#[derive(Clone, Debug, Default)]
pub(crate) struct ExternalDragIntake {
    pub(crate) sources: Vec<ExternalImportSource>,
    pub(crate) screen_pos: Option<egui::Pos2>,
}

pub(crate) fn hovered(ui: &egui::Ui) -> ExternalDragIntake {
    let raw_hovered_files = ui.input(|input| input.raw.hovered_files.clone());
    let sources = merge_sources(
        raw_hovered_files
            .into_iter()
            .filter_map(|file| source_from_hovered_file(file)),
        crate::native_drop::hovered_paths()
            .into_iter()
            .map(ExternalImportSource::Path),
        crate::native_drop::hovered_url_strings()
            .into_iter()
            .map(ExternalImportSource::Url),
    );

    ExternalDragIntake {
        sources,
        screen_pos: resolve_screen_pos(
            crate::native_drop::hovered_screen_pos(),
            crate::native_drop::cursor_screen_pos(),
            ui.ctx().input(|input| input.pointer.latest_pos()),
        ),
    }
}

pub(crate) fn take_drop(ui: &egui::Ui) -> ExternalDragIntake {
    let raw_dropped_files = ui.input(|input| input.raw.dropped_files.clone());
    let sources = merge_sources(
        raw_dropped_files
            .into_iter()
            .filter_map(|file| source_from_dropped_file(file)),
        crate::native_drop::take_dropped_paths()
            .into_iter()
            .map(ExternalImportSource::Path),
        crate::native_drop::take_dropped_url_strings()
            .into_iter()
            .map(ExternalImportSource::Url),
    );

    ExternalDragIntake {
        sources,
        screen_pos: resolve_screen_pos(
            crate::native_drop::take_dropped_screen_pos(),
            crate::native_drop::cursor_screen_pos(),
            ui.ctx().input(|input| input.pointer.latest_pos()),
        ),
    }
}

pub(crate) fn resolve_screen_pos(
    native_drag_pos: Option<egui::Pos2>,
    os_cursor_pos: Option<egui::Pos2>,
    egui_latest_pos: Option<egui::Pos2>,
) -> Option<egui::Pos2> {
    native_drag_pos.or(os_cursor_pos).or(egui_latest_pos)
}

fn source_from_hovered_file(file: egui::HoveredFile) -> Option<ExternalImportSource> {
    let path = file.path?;
    Some(source_from_path_or_url_name(path))
}

fn source_from_dropped_file(file: egui::DroppedFile) -> Option<ExternalImportSource> {
    if let Some(path) = file.path {
        return Some(source_from_path_or_url_name(path));
    }
    normalize_import_url(&file.name).map(ExternalImportSource::Url)
}

fn source_from_path_or_url_name(path: std::path::PathBuf) -> ExternalImportSource {
    let path_text = path.to_string_lossy();
    normalize_import_url(&path_text)
        .map(ExternalImportSource::Url)
        .unwrap_or(ExternalImportSource::Path(path))
}

fn merge_sources(
    egui_sources: impl IntoIterator<Item = ExternalImportSource>,
    native_paths: impl IntoIterator<Item = ExternalImportSource>,
    native_urls: impl IntoIterator<Item = ExternalImportSource>,
) -> Vec<ExternalImportSource> {
    let mut sources = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut seen_urls = HashSet::new();

    for source in egui_sources
        .into_iter()
        .chain(native_paths)
        .chain(native_urls)
    {
        match &source {
            ExternalImportSource::Path(path) if seen_paths.insert(path.clone()) => {
                sources.push(source);
            }
            ExternalImportSource::Url(url) if seen_urls.insert(url.clone()) => {
                sources.push(source);
            }
            _ => {}
        }
    }

    sources
}

#[cfg(test)]
mod tests {
    use super::resolve_screen_pos;

    #[test]
    fn import_screen_pos_prefers_native_drag_then_os_cursor_then_egui_pointer() {
        let native_drag_pos = eframe::egui::pos2(10.0, 20.0);
        let os_cursor_pos = eframe::egui::pos2(30.0, 40.0);
        let egui_latest_pos = eframe::egui::pos2(50.0, 60.0);

        assert_eq!(
            resolve_screen_pos(
                Some(native_drag_pos),
                Some(os_cursor_pos),
                Some(egui_latest_pos),
            ),
            Some(native_drag_pos)
        );
        assert_eq!(
            resolve_screen_pos(None, Some(os_cursor_pos), Some(egui_latest_pos)),
            Some(os_cursor_pos)
        );
        assert_eq!(
            resolve_screen_pos(None, None, Some(egui_latest_pos)),
            Some(egui_latest_pos)
        );
    }
}
