pub(super) use std::fs::{self, File};
pub(super) use std::io::BufWriter;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use eframe::egui;
pub(super) use image::codecs::jpeg::JpegEncoder;
pub(super) use image::codecs::png::{CompressionType, FilterType, PngEncoder};
pub(super) use image::codecs::webp::WebPEncoder;
pub(super) use image::{ColorType, ExtendedColorType, ImageEncoder, ImageFormat};
pub(super) use rusqlite::{Connection, Transaction, params, params_from_iter};

pub(super) use crate::data::{
    Board, BoardItem, BoardItemWithAsset, BoardLayoutItem, CanvasAnnotation, CanvasAnnotationColor,
    CanvasAnnotationKind, CanvasBackground, CanvasImageLod, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, DEFAULT_VIEWPORT_ZOOM, ImageAsset, PlacementContent, PlacementContentIntent,
    PlacementLifecycle, UserSession, ViewportState, WorkspaceSettings,
};

pub(super) fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
