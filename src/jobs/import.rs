use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;
use image::ImageReader;

use crate::data::BoardItemWithAsset;
use crate::storage::Library;

#[derive(Clone, Debug)]
pub enum ImportRequest {
    ImportPath {
        source_path: PathBuf,
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

fn import_path(library: &Library, source_path: PathBuf, desired_world: egui::Pos2) -> ImportResult {
    if !is_png(&source_path) {
        return ImportResult::Failed {
            context: source_path.display().to_string(),
            error: "only PNG files are supported in this first import pass".to_owned(),
        };
    }

    let stored_path = match library.store_original(&source_path) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                context: source_path.display().to_string(),
                error,
            };
        }
    };

    let (rgba, width, height, file_size) = match decode_png(&stored_path) {
        Ok(decoded) => decoded,
        Err(error) => {
            return ImportResult::Failed {
                context: stored_path.display().to_string(),
                error,
            };
        }
    };

    let record =
        match library.persist_imported_image(&stored_path, width, height, file_size, desired_world)
        {
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
    match decode_png(&record.asset.original_path) {
        Ok((rgba, _, _, _)) => ImportResult::Loaded(LoadedCanvasItem { record, rgba }),
        Err(error) => ImportResult::Failed {
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}

fn decode_png(path: &PathBuf) -> Result<(Vec<u8>, u32, u32, u64), String> {
    let file_size = std::fs::metadata(path)
        .map_err(|error| format!("failed to read file metadata: {error}"))?
        .len();
    let image = ImageReader::open(path)
        .map_err(|error| format!("failed to open PNG: {error}"))?
        .decode()
        .map_err(|error| format!("failed to decode PNG: {error}"))?
        .to_rgba8();
    let width = image.width();
    let height = image.height();

    Ok((image.into_raw(), width, height, file_size))
}

fn is_png(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}
