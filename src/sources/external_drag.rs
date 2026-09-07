use std::collections::HashSet;

use eframe::egui;

use crate::image_preparation::ImportSource;
use crate::sources::url::normalize_import_url;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ExternalDragIntake {
    pub(crate) sources: Vec<ImportSource>,
    pub(crate) screen_pos: Option<egui::Pos2>,
}

pub(crate) fn hovered(ui: &egui::Ui) -> ExternalDragIntake {
    let raw_hovered_files = ui.input(|input| input.raw.hovered_files.clone());
    let native = crate::platform::native_drop::hovered();
    let egui_pos = ui.ctx().input(|input| input.pointer.latest_pos());
    let sources = merge_sources(
        raw_hovered_files
            .into_iter()
            .filter_map(|file| file.path.map(source_from_path_or_url_name)),
        native.paths.into_iter().map(ImportSource::Path),
        native.urls.into_iter().map(ImportSource::Url),
    );
    let screen_pos = native.screen_pos.or(egui_pos);

    ExternalDragIntake {
        sources,
        screen_pos,
    }
}

pub(crate) fn take_drop(ui: &egui::Ui) -> ExternalDragIntake {
    let raw_dropped_files = ui.input(|input| input.raw.dropped_files.clone());
    let native = crate::platform::native_drop::take_drop();
    let egui_pos = ui.ctx().input(|input| input.pointer.latest_pos());
    let sources = merge_sources(
        raw_dropped_files
            .into_iter()
            .filter_map(source_from_dropped_file),
        native.paths.into_iter().map(ImportSource::Path),
        native.urls.into_iter().map(ImportSource::Url),
    );
    let screen_pos = native.screen_pos.or(egui_pos);

    ExternalDragIntake {
        sources,
        screen_pos,
    }
}

fn source_from_dropped_file(file: egui::DroppedFile) -> Option<ImportSource> {
    if let Some(path) = file.path {
        return Some(source_from_path_or_url_name(path));
    }
    normalize_import_url(&file.name).map(ImportSource::Url)
}

fn source_from_path_or_url_name(path: std::path::PathBuf) -> ImportSource {
    let path_text = path.to_string_lossy();
    normalize_import_url(&path_text)
        .map(ImportSource::Url)
        .unwrap_or(ImportSource::Path(path))
}

fn merge_sources(
    egui_sources: impl IntoIterator<Item = ImportSource>,
    native_paths: impl IntoIterator<Item = ImportSource>,
    native_urls: impl IntoIterator<Item = ImportSource>,
) -> Vec<ImportSource> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for source in egui_sources
        .into_iter()
        .chain(native_paths)
        .chain(native_urls)
    {
        if seen.insert(source.clone()) {
            sources.push(source);
        }
    }

    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framework_and_native_sources_keep_first_seen_order_without_duplicates() {
        let path = ImportSource::Path("reference.png".into());
        let second = ImportSource::Path("second.png".into());
        let url = ImportSource::Url("https://example.com/reference.png".into());
        assert_eq!(
            merge_sources(
                [path.clone(), url.clone()],
                [path.clone(), second.clone()],
                [url.clone()]
            ),
            vec![path, url, second],
        );
    }
}
