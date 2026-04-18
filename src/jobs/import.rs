use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;
use image::RgbaImage;

use crate::data::BoardItemWithAsset;
use crate::jobs::image_formats::decode_image_file;
use crate::storage::Library;

pub const DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT: f32 = 300.0;

#[derive(Clone, Debug)]
pub enum ImportRequest {
    ImportPath {
        source_path: PathBuf,
        desired_world: egui::Pos2,
        auto_scale: bool,
    },
    ImportClipboard {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
        auto_scale: bool,
    },
    Crop {
        request: crate::canvas::CanvasCropRequest,
    },
    LoadExisting {
        record: BoardItemWithAsset,
    },
}

#[derive(Debug)]
pub enum ImportResult {
    Loaded(LoadedCanvasItem),
    Cropped(CroppedCanvasItem),
    Failed {
        item_id: Option<i64>,
        context: String,
        error: String,
    },
}

#[derive(Debug)]
pub struct LoadedCanvasItem {
    pub record: BoardItemWithAsset,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CroppedCanvasItem {
    pub item_id: i64,
    pub stored_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub rgba: Vec<u8>,
    pub crop_offset_local: egui::Vec2,
}

pub struct ImportWorker {
    sender: Sender<ImportRequest>,
    receiver: Receiver<ImportResult>,
}

impl ImportWorker {
    pub fn new(library: Library) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ImportRequest>();
        let (result_tx, result_rx) = mpsc::channel::<ImportResult>();

        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let result = match request {
                    ImportRequest::ImportPath {
                        source_path,
                        desired_world,
                        auto_scale,
                    } => import_path(&library, source_path, desired_world, auto_scale),
                    ImportRequest::ImportClipboard {
                        rgba,
                        width,
                        height,
                        desired_world,
                        auto_scale,
                    } => import_clipboard(&library, rgba, width, height, desired_world, auto_scale),
                    ImportRequest::Crop { request } => crop_item(&library, request),
                    ImportRequest::LoadExisting { record } => load_existing(record),
                };

                let _ = result_tx.send(result);
            }
        });

        Self {
            sender: request_tx,
            receiver: result_rx,
        }
    }

    pub fn enqueue(&self, request: ImportRequest) -> Result<(), String> {
        self.sender
            .send(request)
            .map_err(|error| format!("failed to enqueue import request: {error}"))
    }

    pub fn try_recv_all(&self) -> Vec<ImportResult> {
        self.receiver.try_iter().collect()
    }
}

pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    auto_scale: bool,
) -> Result<LoadedCanvasItem, String> {
    let decoded = decode_image_file(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let record = library
        .persist_imported_image(
            &stored_path,
            decoded.format.to_mime_type(),
            decoded.width,
            decoded.height,
            decoded.file_size,
            desired_world,
            initial_import_scale(decoded.height, auto_scale),
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;

    Ok(LoadedCanvasItem {
        record,
        rgba: decoded.rgba,
    })
}

fn import_path(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    auto_scale: bool,
) -> ImportResult {
    match import_path_sync(library, source_path.clone(), desired_world, auto_scale) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            item_id: None,
            context: source_path.display().to_string(),
            error,
        },
    }
}

fn import_clipboard(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    auto_scale: bool,
) -> ImportResult {
    let stored_path = match library.store_clipboard_image(&rgba, width, height) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                item_id: None,
                context: "clipboard".to_owned(),
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return ImportResult::Failed {
                item_id: None,
                context: stored_path.display().to_string(),
                error: format!("failed to read clipboard image metadata: {error}"),
            };
        }
    };

    let record = match library.persist_imported_image(
        &stored_path,
        image::ImageFormat::Png.to_mime_type(),
        width,
        height,
        file_size,
        desired_world,
        initial_import_scale(height, auto_scale),
    ) {
        Ok(record) => record,
        Err(error) => {
            return ImportResult::Failed {
                item_id: None,
                context: stored_path.display().to_string(),
                error,
            };
        }
    };

    ImportResult::Loaded(LoadedCanvasItem {
        record,
        rgba,
    })
}

fn load_existing(record: BoardItemWithAsset) -> ImportResult {
    match decode_image_file(&record.asset.original_path) {
        Ok(decoded) => ImportResult::Loaded(LoadedCanvasItem {
            record,
            rgba: decoded.rgba,
        }),
        Err(error) => ImportResult::Failed {
            item_id: None,
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}

fn crop_item(library: &Library, request: crate::canvas::CanvasCropRequest) -> ImportResult {
    let source_context = request.source_asset.original_path.display().to_string();
    let image = match RgbaImage::from_vec(
        request.source_asset.width,
        request.source_asset.height,
        request.source_rgba,
    ) {
        Some(image) => image,
        None => {
            return ImportResult::Failed {
                item_id: Some(request.item_id),
                context: source_context,
                error: "failed to rebuild source image pixels for crop".to_owned(),
            };
        }
    };

    let cropped = image::imageops::crop_imm(
        &image,
        request.crop_rect.x,
        request.crop_rect.y,
        request.crop_rect.width,
        request.crop_rect.height,
    )
    .to_image();
    let cropped_rgba = cropped.into_raw();
    let width = request.crop_rect.width;
    let height = request.crop_rect.height;

    let base_name = std::path::Path::new(&request.source_asset.filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let stored_path = match library.store_rgba_image(
        &format!("{base_name}-crop"),
        &cropped_rgba,
        width,
        height,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                item_id: Some(request.item_id),
                context: stored_path.display().to_string(),
                error: format!("failed to read cropped image metadata: {error}"),
            };
        }
    };

    ImportResult::Cropped(CroppedCanvasItem {
        item_id: request.item_id,
        stored_path,
        width,
        height,
        file_size,
        rgba: cropped_rgba,
        crop_offset_local: request.crop_offset_local,
    })
}

fn initial_import_scale(asset_height: u32, auto_scale: bool) -> f32 {
    if !auto_scale || asset_height == 0 {
        return 1.0;
    }

    (DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT / asset_height as f32).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT, ImportResult, import_clipboard, import_path_sync,
        initial_import_scale,
    };
    use crate::storage::Library;
    use image::{ColorType, ImageFormat};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scales_down_large_portrait_images_to_target_height() {
        let scale = initial_import_scale(1500, true);
        assert!((scale - (DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT / 1500.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn scales_down_large_landscape_images_by_height_cap() {
        let scale = initial_import_scale(1080, true);
        assert!((scale - (DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT / 1080.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn keeps_smaller_images_at_full_scale() {
        assert_eq!(initial_import_scale(240, true), 1.0);
    }

    #[test]
    fn disabling_auto_scale_keeps_full_scale() {
        assert_eq!(initial_import_scale(2000, false), 1.0);
    }

    #[test]
    fn file_imports_land_in_same_height_band() {
        let root = temp_library_root("file-import-scaling");
        let library = Library::initialize(root.clone()).unwrap();

        let portrait_path = write_png(&root, "portrait.png", 1000, 1500);
        let landscape_path = write_png(&root, "landscape.png", 1920, 1080);
        let square_path = write_png(&root, "square.png", 1000, 1000);
        let small_path = write_png(&root, "small.png", 180, 240);

        let portrait = import_path_sync(&library, portrait_path, eframe::egui::Pos2::ZERO, true)
            .unwrap();
        let landscape =
            import_path_sync(&library, landscape_path, eframe::egui::Pos2::ZERO, true).unwrap();
        let square = import_path_sync(&library, square_path, eframe::egui::Pos2::ZERO, true)
            .unwrap();
        let small = import_path_sync(&library, small_path, eframe::egui::Pos2::ZERO, true).unwrap();

        assert!((portrait.record.asset.height as f32 * portrait.record.item.scale - 300.0).abs() < 0.01);
        assert!((landscape.record.asset.height as f32 * landscape.record.item.scale - 300.0).abs() < 0.01);
        assert!((square.record.asset.height as f32 * square.record.item.scale - 300.0).abs() < 0.01);
        assert_eq!(small.record.item.scale, 1.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_import_respects_toggle_and_small_assets_stay_unchanged() {
        let root = temp_library_root("clipboard-import-scaling");
        let library = Library::initialize(root.clone()).unwrap();

        let tall_rgba = solid_rgba(390, 844);
        let small_rgba = solid_rgba(160, 200);

        let tall = match import_clipboard(
            &library,
            tall_rgba,
            390,
            844,
            eframe::egui::Pos2::ZERO,
            true,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded tall clipboard image, got {other:?}"),
        };
        let small = match import_clipboard(
            &library,
            small_rgba,
            160,
            200,
            eframe::egui::Pos2::ZERO,
            false,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded small clipboard image, got {other:?}"),
        };

        assert!((tall.record.asset.height as f32 * tall.record.item.scale - 300.0).abs() < 0.01);
        assert_eq!(small.record.item.scale, 1.0);

        let _ = fs::remove_dir_all(root);
    }

    fn temp_library_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("reference-{label}-{unique}"))
    }

    fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
        let path = root.join(filename);
        let rgba = solid_rgba(width, height);
        image::save_buffer_with_format(
            &path,
            &rgba,
            width,
            height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .unwrap();
        path
    }

    fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
        vec![255; width as usize * height as usize * 4]
    }
}
