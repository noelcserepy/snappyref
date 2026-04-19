use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::RgbaImage;

use crate::data::{BoardItemWithAsset, BoardLayoutItem};
use crate::jobs::image_formats::decode_image_file;
use crate::storage::Library;

pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = 90_000.0;

const PERF_STARTUP_IMAGE_SIZE: u32 = 1536;
const PERF_BACKLOG_IMAGE_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOrigin {
    Existing,
    Path,
    Clipboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportPriority {
    Interactive,
    Background,
}

#[derive(Clone, Copy, Debug)]
pub struct ImportTrace {
    pub queued_at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct StartupPerfReport {
    pub legacy_first_visible_ms: f64,
    pub current_first_visible_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct InteractiveImportPerfReport {
    pub legacy_interactive_import_under_backlog_ms: f64,
    pub current_interactive_import_under_backlog_ms: f64,
}

#[derive(Clone, Debug)]
pub enum ImportRequest {
    ImportPath {
        source_path: PathBuf,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
        auto_scale: bool,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    ImportClipboard {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
        auto_scale: bool,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    Crop {
        request: crate::canvas::CanvasCropRequest,
    },
    LoadExisting {
        record: BoardItemWithAsset,
        trace: Option<ImportTrace>,
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
    pub origin: ImportOrigin,
    pub trace: Option<ImportTrace>,
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
    queue: Arc<RequestQueue>,
    receiver: Receiver<ImportResult>,
}

struct RequestQueue {
    state: Mutex<RequestQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct RequestQueueState {
    interactive: VecDeque<ImportRequest>,
    background: VecDeque<ImportRequest>,
}

impl ImportWorker {
    pub fn new(library: Library) -> Self {
        let (result_tx, result_rx) = mpsc::channel::<ImportResult>();
        let queue = Arc::new(RequestQueue::default());
        let worker_queue = queue.clone();

        thread::spawn(move || {
            loop {
                let request = worker_queue.pop();
                let result = match request {
                    ImportRequest::ImportPath {
                        source_path,
                        desired_world,
                        viewport_world,
                        auto_scale,
                        import_gap,
                        trace,
                    } => import_path(
                        &library,
                        source_path,
                        desired_world,
                        viewport_world,
                        auto_scale,
                        import_gap,
                        trace,
                    ),
                    ImportRequest::ImportClipboard {
                        rgba,
                        width,
                        height,
                        desired_world,
                        viewport_world,
                        auto_scale,
                        import_gap,
                        trace,
                    } => import_clipboard(
                        &library,
                        rgba,
                        width,
                        height,
                        desired_world,
                        viewport_world,
                        auto_scale,
                        import_gap,
                        trace,
                    ),
                    ImportRequest::Crop { request } => crop_item(&library, request),
                    ImportRequest::LoadExisting { record, trace } => load_existing(record, trace),
                };

                let _ = result_tx.send(result);
            }
        });

        Self {
            queue,
            receiver: result_rx,
        }
    }

    pub fn enqueue(&self, request: ImportRequest) -> Result<(), String> {
        self.queue.push(request);
        Ok(())
    }

    pub fn try_recv_all(&self) -> Vec<ImportResult> {
        self.receiver.try_iter().collect()
    }
}

impl Default for RequestQueue {
    fn default() -> Self {
        Self {
            state: Mutex::new(RequestQueueState::default()),
            ready: Condvar::new(),
        }
    }
}

impl RequestQueue {
    fn push(&self, request: ImportRequest) {
        let mut state = self.state.lock().expect("request queue poisoned");
        match request.priority() {
            ImportPriority::Interactive => state.interactive.push_back(request),
            ImportPriority::Background => state.background.push_back(request),
        }
        self.ready.notify_one();
    }

    fn pop(&self) -> ImportRequest {
        let mut state = self.state.lock().expect("request queue poisoned");
        loop {
            if let Some(request) = state.interactive.pop_front() {
                return request;
            }
            if let Some(request) = state.background.pop_front() {
                return request;
            }
            state = self.ready.wait(state).expect("request queue poisoned");
        }
    }
}

impl ImportRequest {
    fn priority(&self) -> ImportPriority {
        match self {
            ImportRequest::LoadExisting { .. } => ImportPriority::Background,
            ImportRequest::ImportPath { .. }
            | ImportRequest::ImportClipboard { .. }
            | ImportRequest::Crop { .. } => ImportPriority::Interactive,
        }
    }
}

#[allow(dead_code)]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    import_path_sync_traced(
        library,
        source_path,
        desired_world,
        viewport_world,
        auto_scale,
        import_gap,
        None,
    )
}

pub fn import_clipboard_sync(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    import_clipboard_sync_traced(
        library,
        rgba,
        width,
        height,
        desired_world,
        viewport_world,
        auto_scale,
        import_gap,
        None,
    )
}

pub fn sort_records_by_viewport_proximity(
    records: &mut [BoardItemWithAsset],
    viewport_world: egui::Rect,
) {
    records.sort_by(|left, right| {
        record_viewport_distance_sq(left, viewport_world)
            .total_cmp(&record_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.item
                    .z_index
                    .cmp(&right.item.z_index)
                    .then_with(|| left.item.id.cmp(&right.item.id))
            })
    });
}

pub fn startup_perf_report() -> Result<StartupPerfReport, String> {
    let root = temp_library_root("startup-perf");
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let paths = [
        write_png(&root, "offscreen-a.png", PERF_STARTUP_IMAGE_SIZE, PERF_STARTUP_IMAGE_SIZE)?,
        write_png(&root, "offscreen-b.png", PERF_STARTUP_IMAGE_SIZE, PERF_STARTUP_IMAGE_SIZE)?,
        write_png(&root, "visible-a.png", PERF_STARTUP_IMAGE_SIZE, PERF_STARTUP_IMAGE_SIZE)?,
        write_png(&root, "visible-b.png", PERF_STARTUP_IMAGE_SIZE, PERF_STARTUP_IMAGE_SIZE)?,
    ];
    let records = vec![
        perf_record(
            1,
            &paths[0],
            egui::pos2(-2200.0, -2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            2,
            &paths[1],
            egui::pos2(2200.0, 2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            3,
            &paths[2],
            egui::pos2(0.0, 0.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            4,
            &paths[3],
            egui::pos2(120.0, 60.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
    ];
    let legacy_first_visible_ms =
        measure_first_visible_load_ms(records.clone(), viewport_world, false)?;
    let current_first_visible_ms =
        measure_first_visible_load_ms(records, viewport_world, true)?;
    let _ = std::fs::remove_dir_all(root);

    Ok(StartupPerfReport {
        legacy_first_visible_ms,
        current_first_visible_ms,
    })
}

pub fn interactive_import_perf_report() -> Result<InteractiveImportPerfReport, String> {
    let root = temp_library_root("interactive-import-perf");
    let library = Library::initialize(root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let mut backlog = Vec::new();
    for index in 0..6 {
        let path = write_png(
            &root,
            &format!("backlog-{index}.png"),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        )?;
        backlog.push(perf_record(
            index as i64 + 1,
            &path,
            egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        ));
    }

    let legacy_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        false,
    )?;
    let current_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        true,
    )?;

    let _ = std::fs::remove_dir_all(root);
    Ok(InteractiveImportPerfReport {
        legacy_interactive_import_under_backlog_ms,
        current_interactive_import_under_backlog_ms,
    })
}

fn import_path_sync_traced(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let decoded = decode_image_file(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;
    let initial_scale = initial_import_scale(decoded.width, decoded.height, auto_scale);
    let arranged_world = arrange_rect_world(
        library,
        desired_world,
        viewport_world,
        decoded.width as f32 * initial_scale.max(0.01),
        decoded.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

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
            arranged_world,
            initial_scale,
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;

    Ok(LoadedCanvasItem {
        record,
        rgba: decoded.rgba,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_path(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
) -> ImportResult {
    match import_path_sync_traced(
        library,
        source_path.clone(),
        desired_world,
        viewport_world,
        auto_scale,
        import_gap,
        trace,
    ) {
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
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
) -> ImportResult {
    match import_clipboard_sync_traced(
        library,
        rgba,
        width,
        height,
        desired_world,
        viewport_world,
        auto_scale,
        import_gap,
        trace,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => {
            ImportResult::Failed {
                item_id: None,
                context: "clipboard".to_owned(),
                error,
            }
        }
    }
}

fn import_clipboard_sync_traced(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let stored_path = library.store_clipboard_image(&rgba, width, height)?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read clipboard image metadata: {error}"))?
        .len();
    let initial_scale = initial_import_scale(width, height, auto_scale);
    let arranged_world = arrange_rect_world(
        library,
        desired_world,
        viewport_world,
        width as f32 * initial_scale.max(0.01),
        height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;
    let record = library.persist_imported_image(
        &stored_path,
        image::ImageFormat::Png.to_mime_type(),
        width,
        height,
        file_size,
        arranged_world,
        initial_scale,
    )?;

    Ok(LoadedCanvasItem {
        record,
        rgba,
        origin: ImportOrigin::Clipboard,
        trace,
    })
}

fn load_existing(record: BoardItemWithAsset, trace: Option<ImportTrace>) -> ImportResult {
    match decode_image_file(&record.asset.original_path) {
        Ok(decoded) => ImportResult::Loaded(LoadedCanvasItem {
            record,
            rgba: decoded.rgba,
            origin: ImportOrigin::Existing,
            trace,
        }),
        Err(error) => ImportResult::Failed {
            item_id: None,
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}

fn measure_first_visible_load_ms(
    mut records: Vec<BoardItemWithAsset>,
    viewport_world: egui::Rect,
    prioritize: bool,
) -> Result<f64, String> {
    if prioritize {
        sort_records_by_viewport_proximity(&mut records, viewport_world);
    }
    let started_at = Instant::now();
    for record in records {
        let loaded = load_existing(record.clone(), None);
        match loaded {
            ImportResult::Loaded(loaded) => {
                if record_intersects_viewport_world(&loaded.record, viewport_world) {
                    return Ok(started_at.elapsed().as_secs_f64() * 1000.0);
                }
            }
            ImportResult::Failed { error, .. } => return Err(error),
            ImportResult::Cropped(_) => {}
        }
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn measure_interactive_import_ms(
    library: &Library,
    backlog: &[BoardItemWithAsset],
    viewport_world: egui::Rect,
    clipboard_rgba: Vec<u8>,
    prioritize: bool,
) -> Result<f64, String> {
    let started_at = Instant::now();
    if prioritize {
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )?;
    } else {
        for record in backlog {
            match load_existing(record.clone(), None) {
                ImportResult::Loaded(_) => {}
                ImportResult::Failed { error, .. } => return Err(error),
                ImportResult::Cropped(_) => {}
            }
        }
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )?;
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn record_viewport_distance_sq(record: &BoardItemWithAsset, viewport_world: egui::Rect) -> f32 {
    let rect = record_world_rect(record);
    if rect.intersects(viewport_world) {
        return 0.0;
    }
    let center = rect.center();
    let clamped = egui::pos2(
        center.x.clamp(viewport_world.left(), viewport_world.right()),
        center.y.clamp(viewport_world.top(), viewport_world.bottom()),
    );
    center.distance_sq(clamped)
}

fn record_intersects_viewport_world(
    record: &BoardItemWithAsset,
    viewport_world: egui::Rect,
) -> bool {
    record_world_rect(record).intersects(viewport_world)
}

fn record_world_rect(record: &BoardItemWithAsset) -> egui::Rect {
    let size = egui::vec2(record.asset.width as f32, record.asset.height as f32)
        * record.item.scale.max(0.01);
    egui::Rect::from_center_size(egui::pos2(record.item.x, record.item.y), size)
}

fn perf_record(
    item_id: i64,
    path: &Path,
    center: egui::Pos2,
    width: u32,
    height: u32,
) -> BoardItemWithAsset {
    BoardItemWithAsset {
        item: crate::data::BoardItem {
            id: item_id,
            asset_id: item_id,
            x: center.x,
            y: center.y,
            scale: 1.0,
            rotation: 0.0,
            z_index: item_id as i32,
            archived: false,
        },
        asset: crate::data::ImageAsset {
            id: item_id,
            original_path: path.to_path_buf(),
            filename: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image.png")
                .to_owned(),
            mime_type: "image/png".to_owned(),
            width,
            height,
            file_size: std::fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
            imported_at: 0,
        },
    }
}

fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let channel = ((x ^ y) & 0xFF) as u8;
            rgba.extend_from_slice(&[channel, channel.wrapping_add(47), 255, 255]);
        }
    }
    rgba
}

fn temp_library_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("reference-{label}-{unique}"))
}

fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create temp perf root: {error}"))?;
    let path = root.join(filename);
    let rgba = solid_rgba(width, height);
    image::save_buffer_with_format(
        &path,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("failed to write perf fixture image: {error}"))?;
    Ok(path)
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

fn initial_import_scale(asset_width: u32, asset_height: u32, auto_scale: bool) -> f32 {
    if !auto_scale || asset_width == 0 || asset_height == 0 {
        return 1.0;
    }

    let asset_area = asset_width as f32 * asset_height as f32;
    (DEFAULT_IMPORT_TARGET_CANVAS_AREA / asset_area)
        .sqrt()
        .min(1.0)
}

#[derive(Clone, Copy, Debug)]
struct LayoutRect {
    id: i64,
    rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct PlacementCandidate {
    center: egui::Pos2,
    generation_index: usize,
}

pub fn arrange_rect_world(
    library: &Library,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> Result<egui::Pos2, String> {
    let new_rect = egui::Rect::from_center_size(
        desired_world,
        egui::vec2(item_width.max(0.0), item_height.max(0.0)),
    );

    let existing_items = library.load_board_layout_items()?;
    if existing_items.is_empty() {
        return Ok(desired_world);
    }

    let mut existing_rects = existing_items
        .iter()
        .map(displayed_layout_rect)
        .collect::<Vec<_>>();
    existing_rects.sort_by(|left, right| {
        left.rect
            .top()
            .total_cmp(&right.rect.top())
            .then_with(|| left.rect.left().total_cmp(&right.rect.left()))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut best_candidate: Option<(f32, f32, f32, usize, egui::Pos2)> = None;
    let mut generation_index = 0_usize;

    for existing in &existing_rects {
        for candidate_rect in [
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.right() + import_gap, existing.rect.top()),
                new_rect.size(),
            ),
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.left(), existing.rect.bottom() + import_gap),
                new_rect.size(),
            ),
        ] {
            let candidate = PlacementCandidate {
                center: candidate_rect.center(),
                generation_index,
            };
            generation_index += 1;

            if candidate_overlaps_any(candidate_rect, &existing_rects, import_gap) {
                continue;
            }

            let viewport_score = viewport_distance_score(candidate_rect.center(), viewport_world);
            let distance_score = candidate.center.distance_sq(desired_world);
            let squareness_score = occupied_bounds_squareness(&existing_rects, candidate_rect);
            let score = (
                viewport_score,
                distance_score,
                squareness_score,
                candidate.generation_index,
                candidate.center,
            );

            match best_candidate {
                Some((best_viewport, best_distance, best_squareness, best_index, _))
                    if viewport_score > best_viewport
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && distance_score > best_distance)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && squareness_score > best_squareness)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && (squareness_score - best_squareness).abs() < 0.001
                            && candidate.generation_index >= best_index) => {}
                _ => best_candidate = Some(score),
            }
        }
    }

    Ok(best_candidate
        .map(|(_, _, _, _, center)| center)
        .unwrap_or(desired_world))
}

fn viewport_distance_score(candidate_center: egui::Pos2, viewport_world: egui::Rect) -> f32 {
    let clamped = egui::pos2(
        candidate_center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        candidate_center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    candidate_center.distance_sq(clamped)
}

fn displayed_layout_rect(record: &BoardLayoutItem) -> LayoutRect {
    let size = egui::vec2(record.width.max(0.0), record.height.max(0.0));
    LayoutRect {
        id: record.id,
        rect: egui::Rect::from_center_size(egui::pos2(record.x, record.y), size),
    }
}

fn candidate_overlaps_any(
    candidate_rect: egui::Rect,
    existing_rects: &[LayoutRect],
    gap: f32,
) -> bool {
    existing_rects
        .iter()
        .any(|existing| rects_overlap_with_gap(candidate_rect, existing.rect, gap))
}

fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() > right.left() - gap
        && left.top() < right.bottom() + gap
        && left.bottom() > right.top() - gap
}

fn occupied_bounds_squareness(existing_rects: &[LayoutRect], candidate_rect: egui::Rect) -> f32 {
    let mut min_x = candidate_rect.left();
    let mut min_y = candidate_rect.top();
    let mut max_x = candidate_rect.right();
    let mut max_y = candidate_rect.bottom();

    for existing in existing_rects {
        min_x = min_x.min(existing.rect.left());
        min_y = min_y.min(existing.rect.top());
        max_x = max_x.max(existing.rect.right());
        max_y = max_y.max(existing.rect.bottom());
    }

    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);
    width.max(height) / width.min(height)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_IMPORT_TARGET_CANVAS_AREA, ImportResult, arrange_rect_world, import_clipboard,
        import_path_sync, initial_import_scale, rects_overlap_with_gap,
    };
    use crate::storage::Library;
    use image::{ColorType, ImageFormat};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scales_down_large_portrait_images_to_target_area() {
        let scale = initial_import_scale(1000, 1500, true);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 1_500_000.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_landscape_images_by_area_cap() {
        let scale = initial_import_scale(1920, 1080, true);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 2_073_600.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn keeps_smaller_images_at_full_scale() {
        assert_eq!(initial_import_scale(180, 240, true), 1.0);
    }

    #[test]
    fn disabling_auto_scale_keeps_full_scale() {
        assert_eq!(initial_import_scale(1200, 2000, false), 1.0);
    }

    #[test]
    fn file_imports_land_in_same_area_band() {
        let root = temp_library_root("file-import-scaling");
        let library = Library::initialize(root.clone()).unwrap();

        let portrait_path = write_png(&root, "portrait.png", 1000, 1500);
        let landscape_path = write_png(&root, "landscape.png", 1920, 1080);
        let square_path = write_png(&root, "square.png", 1000, 1000);
        let small_path = write_png(&root, "small.png", 180, 240);

        let portrait = import_path_sync(
            &library,
            portrait_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let landscape = import_path_sync(
            &library,
            landscape_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let square = import_path_sync(
            &library,
            square_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let small = import_path_sync(
            &library,
            small_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        assert!((displayed_area(&portrait) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert!((displayed_area(&landscape) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert!((displayed_area(&square) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
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
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
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
            test_viewport_world(),
            false,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded small clipboard image, got {other:?}"),
        };

        assert!((displayed_area(&tall) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert_eq!(small.record.item.scale, 1.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_board_placement_uses_requested_anchor() {
        let root = temp_library_root("empty-placement");
        let library = Library::initialize(root.clone()).unwrap();
        let anchor = eframe::egui::pos2(120.0, -48.0);

        let arranged =
            arrange_rect_world(&library, anchor, test_viewport_world(), 300.0, 200.0, 12.0)
                .unwrap();

        assert_eq!(arranged, anchor);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deterministic_candidate_order_prefers_right_before_below_on_tie() {
        let root = temp_library_root("placement-order");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(106.0, 106.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(112.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn blocked_right_side_candidate_falls_through_to_valid_lower_slot() {
        let root = temp_library_root("placement-blocked-right");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);
        seed_board_item(&library, 2, 112.0, 0.0, 100, 100, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(112.0, 50.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(112.0, 112.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn square_scoring_beats_anchor_distance_for_adjacent_imports() {
        let root = temp_library_root("placement-square-scoring");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 240, 80, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(172.0, 0.0),
            test_viewport_world(),
            80.0,
            80.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn viewport_bounding_prefers_on_screen_candidate() {
        let root = temp_library_root("placement-viewport-bounded");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 240, 80, 1.0);
        let viewport = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(100.0, -60.0),
            eframe::egui::pos2(260.0, 60.0),
        );

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(172.0, 0.0),
            viewport,
            80.0,
            80.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sequential_imports_preserve_custom_gap_in_displayed_bounds() {
        let root = temp_library_root("placement-custom-gap");
        let library = Library::initialize(root.clone()).unwrap();
        let gap = 8.0;

        let first = match import_clipboard(
            &library,
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            gap,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected first loaded image, got {other:?}"),
        };

        let second = match import_clipboard(
            &library,
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            gap,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected second loaded image, got {other:?}"),
        };

        let first_rect = eframe::egui::Rect::from_center_size(
            eframe::egui::pos2(first.record.item.x, first.record.item.y),
            eframe::egui::vec2(200.0, 200.0),
        );
        let second_rect = eframe::egui::Rect::from_center_size(
            eframe::egui::pos2(second.record.item.x, second.record.item.y),
            eframe::egui::vec2(200.0, 200.0),
        );

        assert_eq!(second_rect.left() - first_rect.right(), gap);
        assert!(!rects_overlap_with_gap(first_rect, second_rect, gap));
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

    fn displayed_area(loaded: &super::LoadedCanvasItem) -> f32 {
        loaded.record.asset.width as f32
            * loaded.record.asset.height as f32
            * loaded.record.item.scale
            * loaded.record.item.scale
    }

    fn test_viewport_world() -> eframe::egui::Rect {
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(-10_000.0, -10_000.0),
            eframe::egui::pos2(10_000.0, 10_000.0),
        )
    }

    fn seed_board_item(
        library: &Library,
        id: i64,
        x: f32,
        y: f32,
        width: u32,
        height: u32,
        scale: f32,
    ) {
        let stored_path = library
            .store_rgba_image(
                &format!("seed-{id}"),
                &solid_rgba(width, height),
                width,
                height,
            )
            .unwrap();
        let file_size = fs::metadata(&stored_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &stored_path,
                image::ImageFormat::Png.to_mime_type(),
                width,
                height,
                file_size,
                eframe::egui::pos2(x, y),
                scale,
            )
            .unwrap();
        assert_eq!(record.item.id, id);
    }
}
