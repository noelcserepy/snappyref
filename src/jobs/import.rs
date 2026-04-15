use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

use crate::data::BoardItemWithAsset;
use crate::jobs::image_formats::decode_image_file;
use crate::storage::Library;

#[derive(Clone, Debug)]
pub enum ImportRequest {
    ImportPath {
        source_path: PathBuf,
        desired_world: egui::Pos2,
    },
    ImportClipboard {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
    },
    LoadExisting {
        record: BoardItemWithAsset,
    },
}

#[derive(Debug)]
pub enum ImportResult {
    Loaded(LoadedCanvasItem),
    Failed { context: String, error: String },
}

#[derive(Debug)]
pub struct LoadedCanvasItem {
    pub record: BoardItemWithAsset,
    pub rgba: Vec<u8>,
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
                    } => import_path(&library, source_path, desired_world),
                    ImportRequest::ImportClipboard {
                        rgba,
                        width,
                        height,
                        desired_world,
                    } => import_clipboard(&library, rgba, width, height, desired_world),
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
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;

    Ok(LoadedCanvasItem {
        record,
        rgba: decoded.rgba,
    })
}

fn import_path(library: &Library, source_path: PathBuf, desired_world: egui::Pos2) -> ImportResult {
    match import_path_sync(library, source_path.clone(), desired_world) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
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
) -> ImportResult {
    let stored_path = match library.store_clipboard_image(&rgba, width, height) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                context: "clipboard".to_owned(),
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return ImportResult::Failed {
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
    ) {
        Ok(record) => record,
        Err(error) => {
            return ImportResult::Failed {
                context: stored_path.display().to_string(),
                error,
            };
        }
    };

    ImportResult::Loaded(LoadedCanvasItem { record, rgba })
}

fn load_existing(record: BoardItemWithAsset) -> ImportResult {
    match decode_image_file(&record.asset.original_path) {
        Ok(decoded) => ImportResult::Loaded(LoadedCanvasItem {
            record,
            rgba: decoded.rgba,
        }),
        Err(error) => ImportResult::Failed {
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}
