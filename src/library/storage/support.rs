pub(super) use std::fs::{self, File};
pub(super) use std::io::{BufReader, BufWriter, Read};
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::time::{SystemTime, UNIX_EPOCH};

pub(super) use eframe::egui;
pub(super) use image::codecs::jpeg::JpegEncoder;
pub(super) use image::codecs::png::{CompressionType, FilterType, PngEncoder};
pub(super) use image::codecs::webp::WebPEncoder;
pub(super) use image::{ColorType, ExtendedColorType, ImageEncoder, ImageFormat};
pub(super) use rusqlite::{Connection, Transaction, TransactionBehavior, params, params_from_iter};
pub(super) use sha2::{Digest, Sha256};

pub(super) use crate::data::{
    Asset, Board, CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasBackground,
    CanvasImageLod, CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, ContentResolutionSource,
    DEFAULT_VIEWPORT_ZOOM, Placement, PlacementContent, PlacementLifecycle, PlacementState,
    RectGeometry, UserSession, ViewportState, WorkspaceSettings,
};
pub(super) use crate::placement::arrangement::PlacementArrangementInput;

pub(super) fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
