use std::collections::{HashMap, HashSet};

use eframe::egui;
use serde::{Deserialize, Serialize};
#[cfg(feature = "semantic-search")]
use std::time::Duration;
use std::time::Instant;

#[cfg(feature = "semantic-search")]
use crate::semantic_search::{
    DEFAULT_SEMANTIC_SEARCH_MODEL, IndexResult as SearchIndexResult, SearchResultGroup,
    SemanticSearchConfig, SemanticSearchService,
};

const SIDEBAR_WIDTH: f32 = 268.0;
const SIDEBAR_TOP_MARGIN: f32 = 40.0;
const SIDEBAR_BOTTOM_MARGIN: f32 = 16.0;
const SIDEBAR_HINT_MARGIN: f32 = crate::theme::SPACE_3;
const COPY_PAYLOAD_KIND: &str = "reference-board-items";
const MAX_CROP_UNDO_ENTRIES: usize = 32;
const STARTUP_VIEWPORT_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
const MAX_IMPORT_RESULTS_PER_FRAME: usize = 4;
#[cfg(feature = "semantic-search")]
const SEARCH_DEBOUNCE_MS: f32 = 120.0;
#[cfg(feature = "semantic-search")]
const SEARCH_OVERLAY_MAX_SCREEN_HEIGHT_RATIO: f32 = 0.8;
#[cfg(feature = "semantic-search")]
const SEARCH_OVERLAY_TOP_MARGIN: f32 = 56.0;
#[cfg(feature = "semantic-search")]
const SEARCH_OVERLAY_BOTTOM_MARGIN: f32 = 24.0;
#[cfg(feature = "semantic-search")]
const PALETTE_WIDTH: f32 = 660.0;
#[cfg(feature = "semantic-search")]
const PALETTE_GRID_COLUMNS: usize = 5;
#[cfg(feature = "semantic-search")]
const PALETTE_GRID_MAX_HEIGHT: f32 = 360.0;
#[cfg(feature = "semantic-search")]
const SEARCH_OVERLAY_EMPTY_MAX_WIDTH: f32 = PALETTE_WIDTH;
#[cfg(feature = "semantic-search")]
const SEARCH_RESULT_TILE_PREVIEW_HEIGHT: f32 = 144.0;
#[cfg(feature = "semantic-search")]
const SEARCH_RESULT_TILE_FOOTER_HEIGHT: f32 = 0.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrangementAction {
    Compact,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SupportedShortcutAction {
    DeleteSelection,
    DuplicateSelection,
    ToggleSidebar,
    FocusSelection,
    Arrange(ArrangementAction),
    ApplyDefaultAverageArea,
    AverageSelectionArea,
}

#[derive(Clone, Copy)]
struct ShortcutRow {
    action: &'static str,
    windows: &'static str,
    mac: &'static str,
}

#[derive(Clone, Copy)]
struct ShortcutBinding {
    key: egui::Key,
    command: bool,
    shift: bool,
}

#[derive(Clone, Copy)]
struct SupportedShortcut {
    action: &'static str,
    windows: &'static str,
    mac: &'static str,
    binding: Option<ShortcutBinding>,
    shortcut_action: Option<SupportedShortcutAction>,
}

const SUPPORTED_SHORTCUTS: &[SupportedShortcut] = &[
    SupportedShortcut {
        action: "Copy selection",
        windows: "Ctrl+C",
        mac: "⌘C",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Paste image",
        windows: "Ctrl+V",
        mac: "⌘V",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Search images",
        windows: "Ctrl+F",
        mac: "⌘F",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Crop image",
        windows: "C + Drag",
        mac: "C + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw arrow",
        windows: "A + Drag",
        mac: "A + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw line",
        windows: "L + Drag",
        mac: "L + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Draw textbox",
        windows: "T + Click/Drag",
        mac: "T + Click/Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Focus selection",
        windows: "Space",
        mac: "Space",
        binding: Some(ShortcutBinding {
            key: egui::Key::Space,
            command: false,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::FocusSelection),
    },
    SupportedShortcut {
        action: "Delete selection",
        windows: "Delete",
        mac: "Backspace",
        binding: None,
        shortcut_action: Some(SupportedShortcutAction::DeleteSelection),
    },
    SupportedShortcut {
        action: "Duplicate selection",
        windows: "Ctrl+D",
        mac: "⌘D",
        binding: Some(ShortcutBinding {
            key: egui::Key::D,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::DuplicateSelection),
    },
    SupportedShortcut {
        action: "Toggle sidebar",
        windows: "Ctrl+B",
        mac: "⌘B",
        binding: Some(ShortcutBinding {
            key: egui::Key::B,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleSidebar),
    },
    SupportedShortcut {
        action: "Compact arrange",
        windows: "Ctrl+P",
        mac: "⌘P",
        binding: Some(ShortcutBinding {
            key: egui::Key::P,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(ArrangementAction::Compact)),
    },
    SupportedShortcut {
        action: "Row arrange",
        windows: "Ctrl+H",
        mac: "⌘H",
        binding: Some(ShortcutBinding {
            key: egui::Key::H,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(
            ArrangementAction::Horizontal,
        )),
    },
    SupportedShortcut {
        action: "Column arrange",
        windows: "Ctrl+J",
        mac: "⌘J",
        binding: Some(ShortcutBinding {
            key: egui::Key::J,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::Arrange(
            ArrangementAction::Vertical,
        )),
    },
    SupportedShortcut {
        action: "Default avg area",
        windows: "Ctrl+L",
        mac: "⌘L",
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ApplyDefaultAverageArea),
    },
    SupportedShortcut {
        action: "Selection avg area",
        windows: "Ctrl+Shift+L",
        mac: "⇧⌘L",
        binding: Some(ShortcutBinding {
            key: egui::Key::L,
            command: true,
            shift: true,
        }),
        shortcut_action: Some(SupportedShortcutAction::AverageSelectionArea),
    },
    SupportedShortcut {
        action: "Undo crop",
        windows: "Ctrl+Z",
        mac: "⌘Z",
        binding: Some(ShortcutBinding {
            key: egui::Key::Z,
            command: true,
            shift: false,
        }),
        shortcut_action: None,
    },
];

#[derive(Debug, Serialize, Deserialize)]
struct ClipboardBoardSelection {
    kind: String,
    source_center_x: f32,
    source_center_y: f32,
    item_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
struct CropUndoEntry {
    previous_record: crate::data::BoardItemWithAsset,
    previous_rgba: Vec<u8>,
    previous_preview_width: u32,
    previous_preview_height: u32,
    removed_asset_id: i64,
}

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub screenshot_path: Option<std::path::PathBuf>,
    perf_log: bool,
    perf_report: bool,
    startup_trace: bool,
    #[cfg(debug_assertions)]
    verification_mode: Option<VerificationMode>,
    #[cfg(debug_assertions)]
    verification_search_query: Option<String>,
}

#[derive(Clone, Debug)]
struct PerfLogState {
    app_started_at: Instant,
    startup_logged: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct FramePerfState {
    import_results_drained: usize,
    import_result_drain_ms: f64,
    canvas: crate::canvas::CanvasFrameMetrics,
}

#[cfg(feature = "semantic-search")]
#[derive(Default)]
struct SearchOverlayState {
    open: bool,
    query: String,
    last_submitted_query: String,
    last_query_edit_at: Option<Instant>,
    active_result_index: usize,
    active_instance_index: usize,
    results: Vec<SearchResultGroup>,
    status: SearchOverlayStatus,
    focus_requested: bool,
}

#[cfg(feature = "semantic-search")]
#[derive(Default)]
enum SearchOverlayStatus {
    #[default]
    Idle,
    Loading(String),
    Indexing,
    Ready,
    Error(String),
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerificationMode {
    OpenSidebar,
    OpenSidebarHot,
    OpenSidebarNarrow,
    OpenSidebarBoards,
    OpenPalette,
    DrawArrow,
    EditTextbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarTab {
    Boards,
    Options,
}

impl Default for SidebarTab {
    fn default() -> Self {
        SidebarTab::Options
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut screenshot_path = None;
        let mut perf_log = false;
        let mut perf_report = false;
        let mut startup_trace = false;
        #[cfg(debug_assertions)]
        let mut verification_mode = None;
        #[cfg(debug_assertions)]
        let mut verification_search_query = None;
        let mut arguments = std::env::args().skip(1);

        while let Some(argument) = arguments.next() {
            if argument == "--screenshot" {
                screenshot_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--screenshot=") {
                screenshot_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-log" {
                perf_log = true;
            } else if argument == "--perf-report" {
                perf_report = true;
            } else if argument == "--startup-trace" {
                startup_trace = true;
            } else if cfg!(debug_assertions) && argument == "--verify-sidebar" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenSidebar);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-sidebar-hot" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenSidebarHot);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-sidebar-narrow" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenSidebarNarrow);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-sidebar-boards" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenSidebarBoards);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-palette" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenPalette);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-arrow" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::DrawArrow);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-textbox" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::EditTextbox);
                }
            } else if let Some(query) = argument.strip_prefix("--verify-search=") {
                #[cfg(debug_assertions)]
                {
                    verification_search_query = Some(query.to_owned());
                }
            } else if cfg!(debug_assertions) && argument == "--verify-search" {
                #[cfg(debug_assertions)]
                {
                    verification_search_query = arguments.next();
                }
            }
        }

        Self {
            screenshot_path,
            perf_log,
            perf_report,
            startup_trace,
            #[cfg(debug_assertions)]
            verification_mode,
            #[cfg(debug_assertions)]
            verification_search_query,
        }
    }

    #[cfg(debug_assertions)]
    fn open_sidebar_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(
                VerificationMode::OpenSidebar
                    | VerificationMode::OpenSidebarHot
                    | VerificationMode::OpenSidebarNarrow
                    | VerificationMode::OpenSidebarBoards
            )
        )
    }

    #[cfg(debug_assertions)]
    fn open_sidebar_hot_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::OpenSidebarHot)
        )
    }

    #[cfg(debug_assertions)]
    fn open_sidebar_narrow_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::OpenSidebarNarrow)
        )
    }

    #[cfg(debug_assertions)]
    fn open_sidebar_boards_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::OpenSidebarBoards)
        )
    }

    #[cfg(debug_assertions)]
    fn open_palette_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::OpenPalette))
    }

    #[cfg(debug_assertions)]
    fn draw_arrow_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::DrawArrow))
    }

    #[cfg(debug_assertions)]
    fn edit_textbox_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::EditTextbox))
    }

    #[cfg(debug_assertions)]
    fn verification_search_query(&self) -> Option<&str> {
        self.verification_search_query.as_deref()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn startup_viewport_size(&self) -> egui::Vec2 {
        if self.open_sidebar_narrow_on_start() {
            egui::vec2(780.0, STARTUP_VIEWPORT_SIZE.y)
        } else {
            STARTUP_VIEWPORT_SIZE
        }
    }

    #[cfg(not(debug_assertions))]
    fn open_sidebar_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn open_sidebar_hot_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn open_sidebar_boards_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn open_palette_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn draw_arrow_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn edit_textbox_on_start(&self) -> bool {
        false
    }

    #[cfg(not(debug_assertions))]
    fn verification_search_query(&self) -> Option<&str> {
        None
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn startup_viewport_size(&self) -> egui::Vec2 {
        STARTUP_VIEWPORT_SIZE
    }

    pub fn perf_log_enabled(&self) -> bool {
        self.perf_log
    }

    pub fn perf_report_requested(&self) -> bool {
        self.perf_report
    }

    pub fn startup_trace_requested(&self) -> bool {
        self.startup_trace
    }
}

pub struct ReferenceApp {
    canvas: crate::canvas::CanvasState,
    screenshot_capture: crate::screenshot::ScreenshotCapture,
    window_capture: Option<crate::native_capture::NativeWindowCapture>,
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    library: Option<crate::storage::Library>,
    import_worker: Option<crate::jobs::ImportWorker>,
    crop_undo_stack: Vec<CropUndoEntry>,
    max_texture_side: usize,
    auto_image_scaling: bool,
    import_gap: f32,
    canvas_image_mode: crate::data::CanvasImageMode,
    canvas_image_max_side: u32,
    semantic_search_enabled: bool,
    #[cfg(feature = "semantic-search")]
    semantic_search: Option<SemanticSearchService>,
    #[cfg(feature = "semantic-search")]
    search_overlay: SearchOverlayState,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    status: String,
    perf_log: Option<PerfLogState>,
    frame_perf: FramePerfState,
    pending_cache_backfills: HashSet<(i64, crate::data::CanvasImageCacheTier)>,
    inflight_image_tier_loads: HashMap<i64, crate::data::CanvasImageCacheTier>,
    next_import_queue_id: u64,
    #[cfg(debug_assertions)]
    verification_search_query: Option<String>,
    #[cfg(debug_assertions)]
    sidebar_hot_preview: bool,
}

impl ReferenceApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let library_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("library");
        let screenshot_path = config.screenshot_path.clone();
        let window_capture = if screenshot_path.is_some() {
            None
        } else {
            crate::native_capture::NativeWindowCapture::from_creation_context(cc)
        };
        let max_texture_side = cc.egui_ctx.input(|input| input.max_texture_side.max(1));

        match crate::storage::Library::initialize(library_root) {
            Ok(library) => {
                let import_worker = crate::jobs::ImportWorker::new(
                    library.clone(),
                    cc.egui_ctx.clone(),
                    cc.wgpu_render_state.clone(),
                );
                let status = "Library ready".to_owned();
                let mut app = Self {
                    canvas: crate::canvas::CanvasState::default(),
                    screenshot_capture: crate::screenshot::ScreenshotCapture::new(
                        screenshot_path.clone(),
                    ),
                    window_capture,
                    wgpu_render_state: cc.wgpu_render_state.clone(),
                    library: Some(library.clone()),
                    import_worker: Some(import_worker),
                    crop_undo_stack: Vec::new(),
                    max_texture_side,
                    auto_image_scaling: true,
                    import_gap: crate::data::DEFAULT_IMPORT_GAP,
                    canvas_image_mode: crate::data::CanvasImageMode::default(),
                    canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                    semantic_search_enabled: false,
                    #[cfg(feature = "semantic-search")]
                    semantic_search: None,
                    #[cfg(feature = "semantic-search")]
                    search_overlay: SearchOverlayState::default(),
                    sidebar_open: config.open_sidebar_on_start(),
                    sidebar_tab: if config.open_sidebar_boards_on_start() {
                        SidebarTab::Boards
                    } else {
                        SidebarTab::Options
                    },
                    status,
                    perf_log: config.perf_log_enabled().then(|| PerfLogState {
                        app_started_at: Instant::now(),
                        startup_logged: false,
                    }),
                    frame_perf: FramePerfState::default(),
                    pending_cache_backfills: HashSet::new(),
                    inflight_image_tier_loads: HashMap::new(),
                    next_import_queue_id: 1,
                    #[cfg(debug_assertions)]
                    verification_search_query: None,
                    #[cfg(debug_assertions)]
                    sidebar_hot_preview: config.open_sidebar_hot_on_start(),
                };
                app.load_user_session();
                #[cfg(feature = "semantic-search")]
                app.sync_semantic_search_service();
                app.enqueue_existing_items();
                #[cfg(debug_assertions)]
                if config.draw_arrow_on_start() {
                    app.canvas
                        .apply_user_session(&crate::data::UserSession::default());
                    app.canvas.add_annotation(crate::data::CanvasAnnotation {
                        id: -1,
                        kind: crate::data::CanvasAnnotationKind::Arrow,
                        start_x: -220.0,
                        start_y: -120.0,
                        end_x: 220.0,
                        end_y: 120.0,
                        text: String::new(),
                        text_size: crate::data::CanvasTextSize::default(),
                        archived: false,
                    });
                    app.status = "Added debug arrow".to_owned();
                }
                #[cfg(debug_assertions)]
                if config.edit_textbox_on_start() {
                    app.canvas
                        .apply_user_session(&crate::data::UserSession::default());
                    app.canvas.add_annotation(crate::data::CanvasAnnotation {
                        id: -2,
                        kind: crate::data::CanvasAnnotationKind::Textbox,
                        start_x: -160.0,
                        start_y: -80.0,
                        end_x: 160.0,
                        end_y: 80.0,
                        text: "debug textbox".to_owned(),
                        text_size: crate::data::CanvasTextSize::default(),
                        archived: false,
                    });
                    app.canvas.begin_textbox_edit(-2);
                    app.status = "Editing debug textbox".to_owned();
                }
                #[cfg(debug_assertions)]
                if let Some(query) = config.verification_search_query().map(ToOwned::to_owned) {
                    app.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    app.sync_semantic_search_service();
                    #[cfg(feature = "semantic-search")]
                    {
                        app.search_overlay.open = true;
                        app.search_overlay.focus_requested = true;
                        app.search_overlay.query = query;
                        app.search_overlay.last_query_edit_at = Some(Instant::now());
                    }
                    app.verification_search_query =
                        config.verification_search_query().map(ToOwned::to_owned);
                }
                #[cfg(debug_assertions)]
                if app.sidebar_hot_preview {
                    app.auto_image_scaling = false;
                    app.import_gap = 32.0;
                    app.canvas_image_mode = crate::data::CanvasImageMode::Original;
                    app.canvas_image_max_side = crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE;
                    app.canvas
                        .set_background(crate::data::CanvasBackground::Grid);
                    app.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    app.sync_semantic_search_service();
                }
                #[cfg(debug_assertions)]
                if config.open_palette_on_start() {
                    app.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    {
                        app.sync_semantic_search_service();
                        app.search_overlay.open = true;
                        app.search_overlay.focus_requested = true;
                    }
                }
                app
            }
            Err(error) => Self {
                canvas: crate::canvas::CanvasState::default(),
                screenshot_capture: crate::screenshot::ScreenshotCapture::new(screenshot_path),
                window_capture,
                wgpu_render_state: cc.wgpu_render_state.clone(),
                library: None,
                import_worker: None,
                crop_undo_stack: Vec::new(),
                max_texture_side,
                auto_image_scaling: true,
                import_gap: crate::data::DEFAULT_IMPORT_GAP,
                canvas_image_mode: crate::data::CanvasImageMode::default(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                semantic_search_enabled: false,
                #[cfg(feature = "semantic-search")]
                semantic_search: None,
                #[cfg(feature = "semantic-search")]
                search_overlay: SearchOverlayState::default(),
                sidebar_open: config.open_sidebar_on_start(),
                sidebar_tab: if config.open_sidebar_boards_on_start() {
                    SidebarTab::Boards
                } else {
                    SidebarTab::Options
                },
                status: format!("Storage unavailable: {error}"),
                perf_log: config.perf_log_enabled().then(|| PerfLogState {
                    app_started_at: Instant::now(),
                    startup_logged: false,
                }),
                frame_perf: FramePerfState::default(),
                pending_cache_backfills: HashSet::new(),
                inflight_image_tier_loads: HashMap::new(),
                next_import_queue_id: 1,
                #[cfg(debug_assertions)]
                verification_search_query: None,
                #[cfg(debug_assertions)]
                sidebar_hot_preview: config.open_sidebar_hot_on_start(),
            },
        }
    }

    fn enqueue_existing_items(&mut self) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        match library.load_board_items() {
            Ok(mut records) => {
                let viewport_world = self.startup_viewport_world_rect();
                crate::jobs::sort_records_by_viewport_proximity(&mut records, viewport_world);
                for record in records {
                    if let Err(error) =
                        import_worker.enqueue(crate::jobs::ImportRequest::LoadExisting {
                            record,
                            max_texture_side: self.max_texture_side,
                            canvas_image_settings: crate::jobs::CanvasImageSettings {
                                mode: self.canvas_image_mode,
                                max_side: self.canvas_image_max_side,
                            },
                            target_tier: crate::data::CanvasImageCacheTier::Working,
                            priority: crate::jobs::ExistingLoadPriority::Startup,
                            trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                                queued_at: Instant::now(),
                                timings: crate::jobs::ImportTimings::default(),
                            }),
                        })
                    {
                        self.status = error;
                        break;
                    }
                }
            }
            Err(error) => {
                self.status = error;
            }
        }

        match library.load_canvas_annotations() {
            Ok(annotations) => self.canvas.add_annotations(annotations),
            Err(error) => self.status = error,
        }
    }

    fn load_user_session(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_user_session() {
            Ok(session) => {
                self.canvas.apply_user_session(&session);
                self.auto_image_scaling = session.auto_image_scaling;
                self.import_gap = session.import_gap;
                self.canvas_image_mode = session.canvas_image_mode;
                self.canvas_image_max_side = session.canvas_image_max_side;
                self.semantic_search_enabled = session.semantic_search_enabled;
            }
            Err(error) => self.status = error,
        }
    }

    fn startup_viewport_world_rect(&self) -> egui::Rect {
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, STARTUP_VIEWPORT_SIZE);
        egui::Rect::from_min_max(
            self.canvas.screen_to_world(screen_rect.min, screen_rect),
            self.canvas.screen_to_world(screen_rect.max, screen_rect),
        )
    }

    fn next_import_queue_id(&mut self) -> u64 {
        let queue_id = self.next_import_queue_id;
        self.next_import_queue_id = self.next_import_queue_id.saturating_add(1);
        queue_id
    }

    fn visible_canvas_screen_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let sidebar_rect = self.sidebar_rect(screen_rect);
        let left = if self.sidebar_open {
            sidebar_rect.right()
        } else {
            screen_rect.left()
        };
        egui::Rect::from_min_max(egui::pos2(left, screen_rect.top()), screen_rect.max)
    }

    fn queue_import_placeholder(
        &mut self,
        queue_id: u64,
        label: String,
        desired_world: egui::Pos2,
        width: u32,
        height: u32,
    ) {
        let scale = crate::jobs::initial_import_scale(width, height, self.auto_image_scaling);
        self.canvas.add_import_placeholder(
            queue_id,
            desired_world,
            egui::vec2(width as f32 * scale, height as f32 * scale),
            label,
        );
    }

    fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        if self.import_worker.is_none() {
            return;
        }

        let dropped_files = ui.input(|input| input.raw.dropped_files.clone());
        if dropped_files.is_empty() {
            return;
        }

        let desired_world = self.preferred_import_world(ui.ctx(), screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);

        for dropped_file in dropped_files {
            let Some(source_path) = dropped_file.path else {
                continue;
            };

            let source_path_for_status = source_path.clone();
            let queue_id = self.next_import_queue_id();
            let label = source_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image")
                .to_owned();
            let dimensions = image::image_dimensions(&source_path).ok();
            if let Some((width, height)) = dimensions {
                self.queue_import_placeholder(queue_id, label, desired_world, width, height);
            }

            if let Err(error) = self
                .import_worker
                .as_ref()
                .expect("checked import worker")
                .enqueue(crate::jobs::ImportRequest::ImportPath {
                    queue_id,
                    source_path: source_path.clone(),
                    desired_world,
                    viewport_world,
                    max_texture_side: self.max_texture_side,
                    canvas_image_settings: crate::jobs::CanvasImageSettings {
                        mode: self.canvas_image_mode,
                        max_side: self.canvas_image_max_side,
                    },
                    auto_scale: self.auto_image_scaling,
                    import_gap: self.import_gap,
                    trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                        queued_at: Instant::now(),
                        timings: crate::jobs::ImportTimings::default(),
                    }),
                })
            {
                self.canvas.remove_import_placeholder(queue_id);
                self.status = error;
                continue;
            }

            self.status = format!("Queued import: {}", source_path_for_status.display());
        }
    }

    fn handle_clipboard_paste(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        let Some(paste_text) = paste_text else {
            return;
        };

        if self.handle_internal_clipboard_paste(ctx, screen_rect, &paste_text) {
            return;
        }

        if self.import_worker.is_none() {
            return;
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(error) => {
                self.status = format!("Clipboard unavailable: {error}");
                return;
            }
        };

        let clipboard_image = match clipboard.get_image() {
            Ok(image) => image,
            Err(error) => {
                self.status = format!("Clipboard does not contain an image: {error}");
                return;
            }
        };

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let rgba = clipboard_image.bytes.into_owned();
        let width = clipboard_image.width as u32;
        let height = clipboard_image.height as u32;
        let queue_id = self.next_import_queue_id();
        self.queue_import_placeholder(
            queue_id,
            "Clipboard".to_owned(),
            desired_world,
            width,
            height,
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(crate::jobs::ImportRequest::ImportClipboard {
                queue_id,
                rgba,
                width,
                height,
                desired_world,
                viewport_world,
                max_texture_side: self.max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    mode: self.canvas_image_mode,
                    max_side: self.canvas_image_max_side,
                },
                auto_scale: self.auto_image_scaling,
                import_gap: self.import_gap,
                trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                    queued_at: Instant::now(),
                    timings: crate::jobs::ImportTimings::default(),
                }),
            })
        {
            self.canvas.remove_import_placeholder(queue_id);
            self.status = error;
            return;
        }

        self.status = "Queued clipboard image".to_owned();
    }

    fn handle_clipboard_copy(&mut self, ctx: &egui::Context) {
        let copy_requested = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        });
        if !copy_requested || !self.canvas.has_image_selection() {
            return;
        }

        let Some(source_center) = self.canvas.selected_center_world() else {
            return;
        };

        let payload = ClipboardBoardSelection {
            kind: COPY_PAYLOAD_KIND.to_owned(),
            source_center_x: source_center.x,
            source_center_y: source_center.y,
            item_ids: self.canvas.selected_item_ids(),
        };

        let payload = match serde_json::to_string(&payload) {
            Ok(payload) => payload,
            Err(error) => {
                self.status = format!("Failed to serialize clipboard payload: {error}");
                return;
            }
        };

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(error) => {
                self.status = format!("Clipboard unavailable: {error}");
                return;
            }
        };

        if let Err(error) = clipboard.set_text(payload) {
            self.status = format!("Failed to copy selection: {error}");
            return;
        }

        let noun = if self.canvas.selected_item_ids().len() == 1 {
            "image"
        } else {
            "images"
        };
        self.status = format!("Copied {} {noun}", self.canvas.selected_item_ids().len());
    }

    fn handle_internal_clipboard_paste(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        paste_text: &str,
    ) -> bool {
        let Ok(payload) = serde_json::from_str::<ClipboardBoardSelection>(paste_text) else {
            return false;
        };
        if payload.kind != COPY_PAYLOAD_KIND || payload.item_ids.is_empty() {
            return false;
        }

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return true;
        };

        let anchor_world = self.preferred_import_world(ctx, screen_rect);
        let source_center = egui::pos2(payload.source_center_x, payload.source_center_y);
        let duplicated =
            match library.duplicate_board_items(&payload.item_ids, anchor_world, source_center) {
                Ok(records) => records,
                Err(error) => {
                    self.status = error;
                    return true;
                }
            };

        let mut duplicated_ids = Vec::new();
        for record in duplicated {
            duplicated_ids.push(record.item.id);
            if let Err(error) = self.canvas.add_reference_item(record) {
                self.status = error;
                return true;
            }
        }

        self.canvas.select_items(&duplicated_ids);
        let noun = if duplicated_ids.len() == 1 {
            "image"
        } else {
            "images"
        };
        self.status = format!("Pasted {} {noun}", duplicated_ids.len());
        true
    }

    fn handle_import_results(&mut self, ctx: &egui::Context) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        let drain_started_at = Instant::now();
        let results = import_worker.try_recv_up_to(MAX_IMPORT_RESULTS_PER_FRAME);
        let drained_count = results.len();
        let hit_result_cap = results.len() == MAX_IMPORT_RESULTS_PER_FRAME;

        for result in results {
            match result {
                crate::jobs::ImportResult::Loaded(loaded) => {
                    if let Some(queue_id) = loaded.queue_id {
                        self.canvas.remove_import_placeholder(queue_id);
                    }
                    let origin = loaded.origin;
                    let loaded_tier = loaded.loaded_tier;
                    let loaded_record = loaded.record.clone();
                    let existing_load_diagnostics = loaded.existing_load_diagnostics.clone();
                    self.inflight_image_tier_loads
                        .remove(&loaded.record.item.id);
                    if self
                        .canvas
                        .record_for_item_id(loaded.record.item.id)
                        .is_some_and(|current| current.item.asset_id != loaded.record.item.asset_id)
                    {
                        continue;
                    }
                    let mut trace = loaded.trace;
                    #[cfg(feature = "semantic-search")]
                    let queued_asset = (self.semantic_search_enabled
                        && self.semantic_search.is_some())
                    .then(|| {
                        (
                            loaded.record.asset.id,
                            loaded.record.asset.original_path.clone(),
                        )
                    });
                    let ui_integration_started_at = Instant::now();
                    self.canvas.add_loaded_item(ctx, loaded);
                    #[cfg(feature = "semantic-search")]
                    if let Some((asset_id, original_path)) = queued_asset
                        && let Some(service) = &self.semantic_search
                    {
                        let _ = service.enqueue_asset_index(asset_id, original_path);
                        self.search_overlay.status = SearchOverlayStatus::Indexing;
                    }
                    if let Some(trace) = &mut trace {
                        trace.timings.ui_integration = ui_integration_started_at.elapsed();
                    }
                    self.log_loaded_timing(
                        origin,
                        Some(&loaded_record),
                        existing_load_diagnostics.as_ref(),
                        trace,
                    );
                    if loaded_tier == crate::data::CanvasImageCacheTier::Working {
                        self.enqueue_cache_backfill(
                            &loaded_record,
                            crate::data::CanvasImageCacheTier::Overview,
                        );
                    }
                    self.status = "Imported image".to_owned();
                }
                crate::jobs::ImportResult::Cropped(cropped) => {
                    let previous_snapshot = match self.canvas.snapshot_item_image(cropped.item_id) {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            let _ = std::fs::remove_file(&cropped.stored_path);
                            self.canvas.clear_crop_pending(cropped.item_id);
                            self.status = error;
                            continue;
                        }
                    };
                    let Some(library) = &self.library else {
                        let _ = std::fs::remove_file(&cropped.stored_path);
                        self.canvas.clear_crop_pending(cropped.item_id);
                        self.status = "Storage unavailable".to_owned();
                        continue;
                    };

                    let mut updated_item = match self.canvas.projected_cropped_item(&cropped) {
                        Ok(item) => item,
                        Err(error) => {
                            let _ = std::fs::remove_file(&cropped.stored_path);
                            self.canvas.clear_crop_pending(cropped.item_id);
                            self.status = error;
                            continue;
                        }
                    };

                    let asset = match library.create_asset_and_relink_board_item(
                        updated_item.id,
                        &cropped.stored_path,
                        image::ImageFormat::Png.to_mime_type(),
                        cropped.width,
                        cropped.height,
                        cropped.file_size,
                        egui::pos2(updated_item.x, updated_item.y),
                    ) {
                        Ok(asset) => asset,
                        Err(error) => {
                            let _ = std::fs::remove_file(&cropped.stored_path);
                            self.canvas.clear_crop_pending(cropped.item_id);
                            self.status = error;
                            continue;
                        }
                    };

                    let _ = library.store_canvas_image_cache(
                        asset.id,
                        self.canvas_image_mode,
                        self.canvas_image_max_side,
                        &cropped.canvas_rgba,
                        cropped.canvas_width,
                        cropped.canvas_height,
                    );
                    updated_item.asset_id = asset.id;

                    let updated_item_id = updated_item.id;
                    let updated_record = crate::data::BoardItemWithAsset {
                        item: updated_item.clone(),
                        asset: asset.clone(),
                    };
                    match self
                        .canvas
                        .apply_cropped_item(ctx, cropped, asset.clone(), updated_item)
                    {
                        Ok(()) => {
                            self.inflight_image_tier_loads.remove(&updated_item_id);
                            self.canvas.clear_pending_image_tier_load(updated_item_id);
                            self.push_crop_undo_entry(CropUndoEntry {
                                previous_record: previous_snapshot.0,
                                previous_rgba: previous_snapshot.1,
                                previous_preview_width: previous_snapshot.2,
                                previous_preview_height: previous_snapshot.3,
                                removed_asset_id: asset.id,
                            });
                            self.canvas.clear_crop_pending(updated_item_id);
                            #[cfg(feature = "semantic-search")]
                            if self.semantic_search_enabled
                                && let Some(service) = &self.semantic_search
                            {
                                let _ = service
                                    .enqueue_asset_index(asset.id, asset.original_path.clone());
                                self.search_overlay.status = SearchOverlayStatus::Indexing;
                            }
                            self.enqueue_cache_backfill(
                                &updated_record,
                                crate::data::CanvasImageCacheTier::Overview,
                            );
                            self.status = "Cropped image".to_owned();
                        }
                        Err(error) => {
                            self.canvas.clear_crop_pending(updated_item_id);
                            self.status = error;
                        }
                    }
                }
                crate::jobs::ImportResult::CacheTierBackfilled { asset_id, tier } => {
                    self.pending_cache_backfills.remove(&(asset_id, tier));
                }
                crate::jobs::ImportResult::CacheTierBackfillFailed {
                    asset_id,
                    tier,
                    error,
                } => {
                    self.pending_cache_backfills.remove(&(asset_id, tier));
                    self.status = error;
                }
                crate::jobs::ImportResult::Failed {
                    queue_id,
                    item_id,
                    context,
                    error,
                } => {
                    if let Some(queue_id) = queue_id {
                        self.canvas.remove_import_placeholder(queue_id);
                    }
                    if let Some(item_id) = item_id {
                        self.inflight_image_tier_loads.remove(&item_id);
                        self.canvas.clear_crop_pending(item_id);
                        self.canvas.clear_pending_image_tier_load(item_id);
                    }
                    self.status = format!("{context}: {error}");
                }
            }
        }

        self.frame_perf.import_results_drained = drained_count;
        self.frame_perf.import_result_drain_ms = drain_started_at.elapsed().as_secs_f64() * 1000.0;
        if drained_count > 0 && self.perf_log.is_some() {
            eprintln!(
                "[perf] import_result_drain count={} drain_ms={:.2} last_visible={} last_draw_calls={} last_groups={} last_fallback={}",
                drained_count,
                self.frame_perf.import_result_drain_ms,
                self.frame_perf.canvas.image_pass.visible_images,
                self.frame_perf.canvas.image_pass.draw_calls,
                self.frame_perf.canvas.image_pass.texture_groups,
                self.frame_perf.canvas.image_pass.fallback_images,
            );
        }

        if hit_result_cap {
            ctx.request_repaint();
        }
    }

    fn handle_crop_undo(&mut self, ctx: &egui::Context) {
        let undo_requested = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        if !undo_requested {
            return;
        }

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };
        let Some(entry) = self.crop_undo_stack.pop() else {
            return;
        };

        let removed_path = match library.restore_board_item_and_maybe_remove_asset(
            &entry.previous_record.item,
            entry.removed_asset_id,
        ) {
            Ok(path) => path,
            Err(error) => {
                self.crop_undo_stack.push(entry);
                self.status = error;
                return;
            }
        };

        if let Some(path) = removed_path {
            let _ = std::fs::remove_file(&path);
            let _ = library.remove_asset_canvas_caches(entry.removed_asset_id);
            self.canvas.evict_cached_asset(entry.removed_asset_id);
        }

        self.canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                queue_id: None,
                preview_width: entry.previous_preview_width,
                preview_height: entry.previous_preview_height,
                record: entry.previous_record,
                preview_rgba: entry.previous_rgba,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        self.status = "Undid crop".to_owned();
    }

    fn handle_selection_shortcuts(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        #[cfg(feature = "semantic-search")]
        if self.search_overlay.open {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        for shortcut in SUPPORTED_SHORTCUTS {
            let Some(shortcut_action) = shortcut.shortcut_action else {
                continue;
            };
            let triggered = match shortcut_action {
                SupportedShortcutAction::DeleteSelection => delete_shortcut_pressed(ctx),
                SupportedShortcutAction::FocusSelection
                | SupportedShortcutAction::DuplicateSelection
                | SupportedShortcutAction::ToggleSidebar
                | SupportedShortcutAction::ApplyDefaultAverageArea
                | SupportedShortcutAction::AverageSelectionArea
                | SupportedShortcutAction::Arrange(_) => shortcut
                    .binding
                    .is_some_and(|binding| shortcut_pressed(ctx, binding)),
            };
            if !triggered {
                continue;
            }

            match shortcut_action {
                SupportedShortcutAction::DeleteSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    delete_selected_items(
                        &mut self.canvas,
                        self.library.as_ref(),
                        &mut self.status,
                    );
                }
                SupportedShortcutAction::DuplicateSelection => {
                    if !self.canvas.has_image_selection() {
                        continue;
                    }
                    self.duplicate_selection_with_gap(ctx, screen_rect);
                }
                SupportedShortcutAction::FocusSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    let visible_rect = self.visible_canvas_screen_rect(screen_rect);
                    if self
                        .canvas
                        .focus_selection_in_view(screen_rect, visible_rect)
                    {
                        self.status = "Focused selection".to_owned();
                    }
                }
                SupportedShortcutAction::ToggleSidebar => {
                    self.sidebar_open = !self.sidebar_open;
                }
                SupportedShortcutAction::Arrange(action) => {
                    if self.canvas.selected_item_ids().len() > 1 {
                        apply_arrangement_action(
                            &mut self.canvas,
                            action,
                            self.import_gap,
                            &mut self.status,
                        );
                    }
                }
                SupportedShortcutAction::ApplyDefaultAverageArea => {
                    if !self.canvas.has_image_selection() {
                        continue;
                    }
                    apply_default_average_area_to_selection(&mut self.canvas, &mut self.status);
                }
                SupportedShortcutAction::AverageSelectionArea => {
                    if self.canvas.selected_item_ids().len() < 2 {
                        self.status = "Select 2 or more images to average their area".to_owned();
                        continue;
                    }
                    apply_average_selection_area(&mut self.canvas, &mut self.status);
                }
            }
            break;
        }
    }

    fn duplicate_selection_with_gap(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };
        let Some(source_center) = self.canvas.selected_center_world() else {
            return;
        };
        let Some(source_bounds) = self.canvas.selected_bounds_world() else {
            return;
        };

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let anchor_world = match crate::jobs::arrange_rect_world(
            library,
            desired_world,
            viewport_world,
            source_bounds.width(),
            source_bounds.height(),
            self.import_gap,
        ) {
            Ok(anchor_world) => anchor_world,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let duplicated = match library.duplicate_board_items(
            &self.canvas.selected_item_ids(),
            anchor_world,
            source_center,
        ) {
            Ok(records) => records,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        let mut duplicated_ids = Vec::new();
        for record in duplicated {
            duplicated_ids.push(record.item.id);
            if let Err(error) = self.canvas.add_reference_item(record) {
                self.status = error;
                return;
            }
        }

        self.canvas.select_items(&duplicated_ids);
        let noun = if duplicated_ids.len() == 1 {
            "image"
        } else {
            "images"
        };
        self.status = format!("Duplicated {} {noun}", duplicated_ids.len());
    }

    fn handle_pending_crop_requests(&mut self) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_crop() {
            let item_id = request.item_id;
            self.canvas.mark_crop_pending(item_id);
            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::Crop {
                request,
                max_texture_side: self.max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    mode: self.canvas_image_mode,
                    max_side: self.canvas_image_max_side,
                },
            }) {
                self.canvas.clear_crop_pending(item_id);
                self.status = error;
                continue;
            }

            self.status = "Cropping image".to_owned();
        }
    }

    fn enqueue_cache_backfill(
        &mut self,
        record: &crate::data::BoardItemWithAsset,
        tier: crate::data::CanvasImageCacheTier,
    ) {
        if self.canvas_image_mode != crate::data::CanvasImageMode::Downscale {
            return;
        }
        let Some(import_worker) = &self.import_worker else {
            return;
        };
        if !self.pending_cache_backfills.insert((record.asset.id, tier)) {
            return;
        }
        if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::BackfillTier {
            record: record.clone(),
            canvas_image_settings: crate::jobs::CanvasImageSettings {
                mode: self.canvas_image_mode,
                max_side: self.canvas_image_max_side,
            },
            tier,
        }) {
            self.pending_cache_backfills
                .remove(&(record.asset.id, tier));
            self.status = error;
        }
    }

    fn handle_pending_image_tier_requests(&mut self) {
        if self.canvas_image_mode != crate::data::CanvasImageMode::Downscale {
            return;
        }
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        for (item_id, target_tier) in self.canvas.take_pending_image_tier_loads() {
            if self
                .inflight_image_tier_loads
                .get(&item_id)
                .is_some_and(|existing| *existing >= target_tier)
            {
                continue;
            }
            let Some(record) = self.canvas.record_for_item_id(item_id) else {
                continue;
            };
            self.inflight_image_tier_loads.insert(item_id, target_tier);
            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::LoadExisting {
                record,
                max_texture_side: self.max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    mode: self.canvas_image_mode,
                    max_side: self.canvas_image_max_side,
                },
                target_tier,
                priority: crate::jobs::ExistingLoadPriority::Background,
                trace: None,
            }) {
                self.inflight_image_tier_loads.remove(&item_id);
                self.canvas.clear_pending_image_tier_load(item_id);
                self.status = error;
            }
        }
    }

    fn handle_pending_annotation_requests(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_annotation() {
            match library.create_canvas_annotation(
                request.kind,
                request.start_world,
                request.end_world,
                &request.text,
                request.text_size,
            ) {
                Ok(annotation) => {
                    let should_focus_text =
                        annotation.kind == crate::data::CanvasAnnotationKind::Textbox;
                    let annotation_id = annotation.id;
                    self.canvas.add_annotation(annotation);
                    if should_focus_text {
                        self.canvas.begin_textbox_edit(annotation_id);
                    }
                    self.status = match request.kind {
                        crate::data::CanvasAnnotationKind::Arrow => "Added arrow".to_owned(),
                        crate::data::CanvasAnnotationKind::Line => "Added line".to_owned(),
                        crate::data::CanvasAnnotationKind::Textbox => "Added textbox".to_owned(),
                    };
                }
                Err(error) => {
                    self.status = error;
                }
            }
        }
    }

    fn persist_canvas_changes(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        let archived_annotation_ids = self.canvas.take_archived_annotation_ids();
        if !archived_annotation_ids.is_empty()
            && let Err(error) = library.archive_canvas_annotations(&archived_annotation_ids)
        {
            self.status = error;
        }

        for item in self.canvas.take_dirty_items() {
            if let Err(error) = library.update_board_item(&item) {
                self.status = error;
            }
        }

        for annotation in self.canvas.take_dirty_annotations() {
            if let Err(error) = library.update_canvas_annotation(&annotation) {
                self.status = error;
            }
        }
    }

    fn save_user_session(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        let mut session = self.canvas.user_session();
        session.auto_image_scaling = self.auto_image_scaling;
        session.import_gap = self.import_gap;
        session.canvas_image_mode = self.canvas_image_mode;
        session.canvas_image_max_side = self.canvas_image_max_side;
        session.semantic_search_enabled = self.semantic_search_enabled;
        if let Err(error) = library.save_user_session(&session) {
            self.status = error;
        }
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn drive_verification_search(&mut self) {
        let Some(query) = self.verification_search_query.clone() else {
            return;
        };

        if !self.semantic_search_enabled {
            self.semantic_search_enabled = true;
            self.sync_semantic_search_service();
        }

        self.search_overlay.open = true;
        self.search_overlay.focus_requested = true;
        if self.search_overlay.query.trim() != query.trim() {
            self.search_overlay.query = query.clone();
            self.search_overlay.last_query_edit_at = None;
            self.search_overlay.last_submitted_query.clear();
            self.search_overlay.results.clear();
            self.canvas.clear_search_matches();
        }

        self.submit_search_query();
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn verification_search_settled(&self) -> bool {
        let Some(query) = self.verification_search_query.as_deref() else {
            return true;
        };

        self.search_overlay.open
            && self.search_overlay.query.trim() == query.trim()
            && self.search_overlay.last_submitted_query == query.trim()
            && matches!(
                self.search_overlay.status,
                SearchOverlayStatus::Ready | SearchOverlayStatus::Error(_)
            )
    }

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn drive_verification_search(&mut self) {}

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn verification_search_settled(&self) -> bool {
        true
    }

    #[cfg(feature = "semantic-search")]
    fn sync_semantic_search_service(&mut self) {
        if !self.semantic_search_enabled {
            self.semantic_search = None;
            self.close_search_overlay();
            return;
        }

        if self.semantic_search.is_some() {
            return;
        }

        let Some(library) = &self.library else {
            self.search_overlay.status =
                SearchOverlayStatus::Error("Storage unavailable".to_owned());
            return;
        };

        match SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: true,
                model: DEFAULT_SEMANTIC_SEARCH_MODEL,
                max_results: 5,
            },
        ) {
            Ok(service) => {
                self.search_overlay.status =
                    SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                let _ = service.enqueue_startup_backfill();
                self.semantic_search = Some(service);
            }
            Err(error) => {
                self.search_overlay.status = SearchOverlayStatus::Error(error.clone());
                self.status = format!("Semantic search unavailable: {error}");
            }
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_results(&mut self) {
        let Some(service) = &self.semantic_search else {
            return;
        };

        for result in service.try_recv_all() {
            match result {
                SearchIndexResult::Progress { message } => {
                    if !matches!(self.search_overlay.status, SearchOverlayStatus::Error(_)) {
                        self.search_overlay.status = SearchOverlayStatus::Loading(message);
                    }
                }
                SearchIndexResult::ModelReady => {
                    if !self.search_overlay.query.trim().is_empty() {
                        self.search_overlay.last_submitted_query.clear();
                    }
                    if !matches!(self.search_overlay.status, SearchOverlayStatus::Error(_)) {
                        self.search_overlay.status = SearchOverlayStatus::Ready;
                    }
                }
                SearchIndexResult::Indexed => {
                    if !self.search_overlay.query.trim().is_empty() {
                        self.search_overlay.last_submitted_query.clear();
                    }
                }
                SearchIndexResult::Failed { error } => {
                    self.search_overlay.status = SearchOverlayStatus::Error(error);
                }
            }
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_shortcut(&mut self, ctx: &egui::Context) {
        if !self.semantic_search_enabled {
            return;
        }

        let triggered =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::F));
        if !triggered {
            return;
        }

        self.sync_semantic_search_service();
        self.search_overlay.open = true;
        self.search_overlay.focus_requested = true;
        if !matches!(self.search_overlay.status, SearchOverlayStatus::Error(_)) {
            let status = self
                .semantic_search
                .as_ref()
                .map(|service| {
                    if service.is_ready() {
                        SearchOverlayStatus::Ready
                    } else {
                        SearchOverlayStatus::Loading("Preparing semantic search…".to_owned())
                    }
                })
                .unwrap_or_else(|| {
                    SearchOverlayStatus::Loading("Preparing semantic search…".to_owned())
                });
            self.search_overlay.status = status;
        }
    }

    #[cfg(feature = "semantic-search")]
    fn close_search_overlay(&mut self) {
        self.search_overlay = SearchOverlayState::default();
        self.canvas.clear_search_matches();
    }

    #[cfg(feature = "semantic-search")]
    fn submit_search_query(&mut self) {
        let query = self.search_overlay.query.trim().to_owned();
        if query.is_empty() {
            self.search_overlay.results.clear();
            self.search_overlay.last_submitted_query.clear();
            self.search_overlay.active_result_index = 0;
            self.search_overlay.active_instance_index = 0;
            self.search_overlay.status = SearchOverlayStatus::Ready;
            self.canvas.clear_search_matches();
            return;
        }

        let Some(service) = &self.semantic_search else {
            self.search_overlay.status =
                SearchOverlayStatus::Error("Semantic search unavailable".to_owned());
            return;
        };

        match service.search(&query) {
            Ok(results) => {
                self.search_overlay.last_submitted_query = query;
                self.search_overlay.results = results;
                self.search_overlay.active_result_index = 0;
                self.search_overlay.active_instance_index = 0;
                self.search_overlay.status = SearchOverlayStatus::Ready;
                self.apply_search_matches();
            }
            Err(error) if error == "Loading model…" => {
                if !matches!(self.search_overlay.status, SearchOverlayStatus::Loading(_)) {
                    self.search_overlay.status =
                        SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                }
            }
            Err(error) => {
                self.search_overlay.status = SearchOverlayStatus::Error(error);
                self.search_overlay.results.clear();
                self.canvas.clear_search_matches();
            }
        }
    }

    #[cfg(feature = "semantic-search")]
    fn apply_search_matches(&mut self) {
        let item_ids = self
            .search_overlay
            .results
            .iter()
            .flat_map(|group| group.item_ids.iter().copied())
            .collect::<Vec<_>>();
        let active_item_id = self
            .search_overlay
            .results
            .get(self.search_overlay.active_result_index)
            .and_then(|group| {
                if group.item_ids.is_empty() {
                    None
                } else {
                    Some(
                        group.item_ids
                            [self.search_overlay.active_instance_index % group.item_ids.len()],
                    )
                }
            });
        self.canvas.set_search_matches(item_ids, active_item_id);
    }

    #[cfg(feature = "semantic-search")]
    fn focus_active_search_result(&mut self) {
        let Some(group) = self
            .search_overlay
            .results
            .get(self.search_overlay.active_result_index)
        else {
            return;
        };
        if group.item_ids.is_empty() {
            return;
        }

        let item_id =
            group.item_ids[self.search_overlay.active_instance_index % group.item_ids.len()];
        self.canvas.focus_item(item_id);
        self.search_overlay.active_instance_index =
            (self.search_overlay.active_instance_index + 1) % group.item_ids.len();
        self.apply_search_matches();
    }

    #[cfg(feature = "semantic-search")]
    fn show_search_overlay(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        if !self.search_overlay.open {
            return;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.close_search_overlay();
            return;
        }

        let input_id = egui::Id::new("semantic_search_input");
        let width = PALETTE_WIDTH;
        let result_count = self.search_overlay.results.len();
        let body_h_pad = 12.0_f32;
        let grid_inner_width = width - body_h_pad * 2.0;
        let grid_layout = search_result_grid_layout(grid_inner_width, result_count);
        let grid_content_height =
            search_result_grid_content_height(result_count, grid_layout.columns);
        let grid_viewport_height = grid_content_height.min(PALETTE_GRID_MAX_HEIGHT);
        let area_response = egui::Area::new(egui::Id::new("semantic_search_overlay"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(
                screen_rect.center().x,
                screen_rect.top() + SEARCH_OVERLAY_TOP_MARGIN,
            ))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_min_width(width);
                ui.set_max_width(width);
                egui::Frame::new()
                    .fill(crate::theme::PALETTE_SURFACE)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(33)))
                    .corner_radius(egui::CornerRadius::same(12))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 40,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(180),
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        let response =
                            paint_palette_input_row(ui, &mut self.search_overlay.query, input_id);
                        if self.search_overlay.focus_requested {
                            response.request_focus();
                            self.search_overlay.focus_requested =
                                !ctx.memory(|memory| memory.has_focus(input_id));
                            if self.search_overlay.focus_requested {
                                ctx.request_repaint_after(Duration::from_millis(16));
                            }
                        }
                        if response.changed() {
                            self.search_overlay.last_query_edit_at = Some(Instant::now());
                            self.search_overlay.active_result_index = 0;
                            self.search_overlay.active_instance_index = 0;
                            if self.search_overlay.query.trim().is_empty() {
                                self.search_overlay.results.clear();
                                self.canvas.clear_search_matches();
                            }
                        }

                        paint_palette_facet_row(ui, result_count);
                        paint_palette_divider(ui);

                        if result_count == 0 {
                            paint_palette_empty_state(ui);
                        } else {
                            let mut clicked_index = None;
                            egui::Frame::new()
                                .inner_margin(egui::Margin {
                                    left: body_h_pad as i8,
                                    right: body_h_pad as i8,
                                    top: body_h_pad as i8,
                                    bottom: body_h_pad as i8,
                                })
                                .show(ui, |ui| {
                                    ui.set_min_width(grid_inner_width);
                                    ui.set_max_width(grid_inner_width);
                                    let mut render_grid = |ui: &mut egui::Ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(
                                            crate::theme::SPACE_2,
                                            crate::theme::SPACE_2,
                                        );
                                        for (row_index, row_groups) in self
                                            .search_overlay
                                            .results
                                            .chunks(grid_layout.columns.max(1))
                                            .enumerate()
                                        {
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x =
                                                    crate::theme::SPACE_2;
                                                for (column_index, group) in
                                                    row_groups.iter().enumerate()
                                                {
                                                    let index = row_index * grid_layout.columns
                                                        + column_index;
                                                    let active = index
                                                        == self
                                                            .search_overlay
                                                            .active_result_index;
                                                    let response = search_result_tile(
                                                        ui,
                                                        &self.canvas,
                                                        group,
                                                        active,
                                                        grid_layout.tile_width,
                                                    );
                                                    if response.clicked() {
                                                        clicked_index = Some(index);
                                                    }
                                                }
                                            });
                                        }
                                    };

                                    if grid_content_height <= PALETTE_GRID_MAX_HEIGHT {
                                        render_grid(ui);
                                    } else {
                                        egui::ScrollArea::vertical()
                                            .max_height(grid_viewport_height)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| render_grid(ui));
                                    }
                                });
                            if let Some(index) = clicked_index {
                                self.search_overlay.active_result_index = index;
                                self.search_overlay.active_instance_index = 0;
                                self.apply_search_matches();
                            }
                        }

                        paint_palette_divider(ui);
                        paint_palette_footer(ui, result_count, &self.search_overlay.status);
                    });
            });

        let overlay_rect = area_response.response.rect;
        let clicked_outside = ctx.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pointer_pos| !overlay_rect.contains(pointer_pos))
        });
        if clicked_outside {
            self.close_search_overlay();
            return;
        }

        if let Some(changed_at) = self.search_overlay.last_query_edit_at
            && changed_at.elapsed().as_secs_f32() * 1000.0 >= SEARCH_DEBOUNCE_MS
            && self.search_overlay.query.trim() != self.search_overlay.last_submitted_query
        {
            self.submit_search_query();
        } else if self.search_overlay.last_query_edit_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        if !self.search_overlay.results.is_empty() {
            let last_index = self.search_overlay.results.len() - 1;
            if ctx.input(|input| input.key_pressed(egui::Key::ArrowRight)) {
                self.search_overlay.active_result_index =
                    (self.search_overlay.active_result_index + 1).min(last_index);
                self.search_overlay.active_instance_index = 0;
                self.apply_search_matches();
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft)) {
                self.search_overlay.active_result_index =
                    self.search_overlay.active_result_index.saturating_sub(1);
                self.search_overlay.active_instance_index = 0;
                self.apply_search_matches();
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                self.search_overlay.active_result_index =
                    (self.search_overlay.active_result_index + grid_layout.columns).min(last_index);
                self.search_overlay.active_instance_index = 0;
                self.apply_search_matches();
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                self.search_overlay.active_result_index = self
                    .search_overlay
                    .active_result_index
                    .saturating_sub(grid_layout.columns);
                self.search_overlay.active_instance_index = 0;
                self.apply_search_matches();
            }

            if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.focus_active_search_result();
            }
        }
    }

    fn persist_settings_if_changed(&mut self, settings_changed: bool) {
        Self::persist_settings_if_changed_with(settings_changed, || self.save_user_session());
    }

    fn persist_settings_if_changed_with<F>(settings_changed: bool, mut persist: F)
    where
        F: FnMut(),
    {
        if settings_changed {
            persist();
        }
    }

    fn preferred_import_world(&self, ctx: &egui::Context, screen_rect: egui::Rect) -> egui::Pos2 {
        let pointer_screen_pos = ctx.input(|input| input.pointer.latest_pos());
        self.preferred_import_world_with_pointer(screen_rect, pointer_screen_pos)
    }

    fn import_viewport_world_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let visible_screen_rect = self.visible_canvas_screen_rect(screen_rect);
        egui::Rect::from_two_pos(
            self.canvas
                .screen_to_world(visible_screen_rect.min, screen_rect),
            self.canvas
                .screen_to_world(visible_screen_rect.max, screen_rect),
        )
    }

    fn preferred_import_world_with_pointer(
        &self,
        screen_rect: egui::Rect,
        pointer_screen_pos: Option<egui::Pos2>,
    ) -> egui::Pos2 {
        let visible_rect = self.visible_canvas_screen_rect(screen_rect);
        let canvas_center = visible_rect.center();
        let import_screen_pos = pointer_screen_pos
            .filter(|screen_pos| visible_rect.contains(*screen_pos))
            .unwrap_or(canvas_center);
        self.canvas.screen_to_world(import_screen_pos, screen_rect)
    }

    fn sidebar_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let top = screen_rect.top() + SIDEBAR_TOP_MARGIN;
        let bottom = (screen_rect.bottom() - SIDEBAR_BOTTOM_MARGIN).max(top);
        egui::Rect::from_min_max(
            egui::pos2(screen_rect.left(), top),
            egui::pos2(screen_rect.left() + SIDEBAR_WIDTH, bottom),
        )
    }

    fn push_crop_undo_entry(&mut self, entry: CropUndoEntry) {
        self.crop_undo_stack.push(entry);
        if self.crop_undo_stack.len() > MAX_CROP_UNDO_ENTRIES {
            self.crop_undo_stack.remove(0);
        }
    }

    fn log_loaded_timing(
        &mut self,
        origin: crate::jobs::ImportOrigin,
        record: Option<&crate::data::BoardItemWithAsset>,
        diagnostics: Option<&crate::jobs::ExistingLoadDiagnostics>,
        trace: Option<crate::jobs::ImportTrace>,
    ) {
        let Some(perf_log) = &mut self.perf_log else {
            return;
        };

        let format_trace = |trace: crate::jobs::ImportTrace| {
            format!(
                " total_ms={:.2} queue_ms={:.2} decode_ms={:.2} store_ms={:.2} preview_ms={:.2} worker_ms={:.2} ui_ms={:.2}",
                trace.queued_at.elapsed().as_secs_f64() * 1000.0,
                trace.timings.queue_wait.as_secs_f64() * 1000.0,
                trace.timings.decode_or_load.as_secs_f64() * 1000.0,
                trace.timings.store_and_persist.as_secs_f64() * 1000.0,
                trace.timings.preview_prepare.as_secs_f64() * 1000.0,
                trace.timings.worker_total.as_secs_f64() * 1000.0,
                trace.timings.ui_integration.as_secs_f64() * 1000.0,
            )
        };

        match origin {
            crate::jobs::ImportOrigin::Existing if !perf_log.startup_logged => {
                perf_log.startup_logged = true;
                let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
                if let Some(trace) = trace {
                    eprintln!(
                        "[perf] startup_first_image_ms={startup_ms:.2}{}",
                        format_trace(trace)
                    );
                } else {
                    eprintln!("[perf] startup_first_image_ms={startup_ms:.2}");
                }
            }
            crate::jobs::ImportOrigin::Existing => {
                let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
                let asset = record.map(|record| {
                    format!(
                        " item_id={} asset_id={} file={}",
                        record.item.id, record.asset.id, record.asset.filename
                    )
                });
                let source = diagnostics.map(|diagnostics| {
                    format!(
                        " source_kind={:?} source_mb={:.2} backfilled_cache={} source={} source_dims={}x{}",
                        diagnostics.source_kind,
                        diagnostics.source_file_size as f64 / 1024.0 / 1024.0,
                        diagnostics.backfilled_cache,
                        diagnostics.source_path.display(),
                        diagnostics.source_width,
                        diagnostics.source_height
                    )
                });
                let trace_text = trace.map(format_trace).unwrap_or_default();
                eprintln!(
                    "[perf] startup_image_visible startup_ms={startup_ms:.2}{}{}{}{}",
                    asset.unwrap_or_default(),
                    source.unwrap_or_default(),
                    if trace_text.is_empty() { "" } else { " " },
                    trace_text
                );
            }
            crate::jobs::ImportOrigin::Clipboard => {
                if let Some(trace) = trace {
                    eprintln!("[perf] clipboard_import{}", format_trace(trace));
                }
            }
            crate::jobs::ImportOrigin::Path => {
                if let Some(trace) = trace {
                    eprintln!("[perf] dropped_file_import{}", format_trace(trace));
                }
            }
        }
    }
}

impl eframe::App for ReferenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.ctx().viewport_rect();
        ui.set_clip_rect(screen_rect);
        ui.expand_to_include_rect(screen_rect);
        self.frame_perf.import_results_drained = 0;
        self.frame_perf.import_result_drain_ms = 0.0;

        self.handle_import_results(ui.ctx());
        #[cfg(feature = "semantic-search")]
        self.handle_search_results();
        self.handle_crop_undo(ui.ctx());
        self.drive_verification_search();

        if !self.verification_search_settled() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
            return;
        }

        if self.screenshot_capture.is_pending()
            && let Some(render_state) = &self.wgpu_render_state
        {
            let _ = render_state.device.poll(eframe::wgpu::PollType::Poll);
            ui.ctx().request_repaint();
        }

        if self.screenshot_capture.begin_frame(
            ui.ctx(),
            self.window_capture.as_ref(),
            &mut self.status,
        ) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        self.handle_clipboard_copy(ui.ctx());
        self.handle_clipboard_paste(ui.ctx(), screen_rect);
        self.handle_dropped_files(ui, screen_rect);
        #[cfg(feature = "semantic-search")]
        self.handle_search_shortcut(ui.ctx());
        self.handle_selection_shortcuts(ui.ctx(), screen_rect);
        let sidebar_open = self.sidebar_open;
        if sidebar_open {
            block_sidebar_input(ui.ctx(), self.sidebar_rect(screen_rect));
        }
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        self.frame_perf.canvas = crate::canvas::show(
            ui,
            screen_rect,
            visible_canvas_rect,
            &mut self.canvas,
            self.wgpu_render_state.as_ref(),
            self.canvas_image_mode == crate::data::CanvasImageMode::Downscale,
            false,
        );
        self.handle_pending_image_tier_requests();
        self.handle_pending_crop_requests();
        self.handle_pending_annotation_requests();
        self.persist_canvas_changes();
        let sidebar_result = show_sidebar(
            ui.ctx(),
            screen_rect,
            sidebar_open,
            &mut self.sidebar_tab,
            &mut self.auto_image_scaling,
            &mut self.import_gap,
            &mut self.canvas_image_mode,
            &mut self.canvas_image_max_side,
            &mut self.semantic_search_enabled,
            &mut self.canvas,
        );
        if sidebar_open {
            close_sidebar_on_outside_click(
                ui.ctx(),
                sidebar_result
                    .interactive_rect
                    .unwrap_or_else(|| self.sidebar_rect(screen_rect)),
                &mut self.sidebar_open,
            );
        }
        if sidebar_result.settings_changed {
            self.sidebar_open = true;
            #[cfg(feature = "semantic-search")]
            self.sync_semantic_search_service();
        }
        self.persist_settings_if_changed(sidebar_result.settings_changed);
        show_sidebar_hint(ui.ctx(), screen_rect, sidebar_open);
        #[cfg(feature = "semantic-search")]
        self.show_search_overlay(ui.ctx(), screen_rect);
        if self
            .screenshot_capture
            .handle_event(ui.ctx(), &mut self.status)
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self) {
        self.save_user_session();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        crate::theme::BACKGROUND.to_normalized_gamma_f32()
    }
}

fn shortcut_pressed(ctx: &egui::Context, binding: ShortcutBinding) -> bool {
    ctx.input(|input| {
        input.modifiers.command == binding.command
            && input.modifiers.shift == binding.shift
            && input.key_pressed(binding.key)
    })
}

fn delete_shortcut_pressed(ctx: &egui::Context) -> bool {
    #[cfg(target_os = "macos")]
    {
        ctx.input(|input| input.key_pressed(egui::Key::Backspace))
    }

    #[cfg(not(target_os = "macos"))]
    {
        ctx.input(|input| input.key_pressed(egui::Key::Delete))
    }
}

fn delete_selected_items(
    canvas: &mut crate::canvas::CanvasState,
    library: Option<&crate::storage::Library>,
    status: &mut String,
) {
    let annotation_ids = canvas.delete_selected_annotations();
    if !annotation_ids.is_empty() {
        if let Some(library) = library
            && let Err(error) = library.archive_canvas_annotations(&annotation_ids)
        {
            *status = error;
            return;
        }
        let noun = if annotation_ids.len() == 1 {
            "annotation"
        } else {
            "annotations"
        };
        *status = format!("Archived {} {noun}", annotation_ids.len());
    }

    let archived_ids = canvas.delete_selected_images();
    if archived_ids.is_empty() {
        return;
    }

    if let Some(library) = library
        && let Err(error) = library.archive_board_items(&archived_ids)
    {
        *status = error;
        return;
    }

    let noun = if archived_ids.len() == 1 {
        "image"
    } else {
        "images"
    };
    *status = format!("Archived {} {noun}", archived_ids.len());
}

fn apply_arrangement_action(
    canvas: &mut crate::canvas::CanvasState,
    action: ArrangementAction,
    gap: f32,
    status: &mut String,
) {
    let arranged = match action {
        ArrangementAction::Compact => canvas.arrange_selected_compact(gap),
        ArrangementAction::Horizontal => canvas.arrange_selected_horizontal(gap),
        ArrangementAction::Vertical => canvas.arrange_selected_vertical(gap),
    };

    let noun = if arranged == 1 { "image" } else { "images" };
    *status = match action {
        ArrangementAction::Compact => format!("Compacted {arranged} {noun}"),
        ArrangementAction::Horizontal => format!("Arranged {arranged} {noun} in a row"),
        ArrangementAction::Vertical => format!("Arranged {arranged} {noun} in a column"),
    };
}

fn apply_default_average_area_to_selection(
    canvas: &mut crate::canvas::CanvasState,
    status: &mut String,
) {
    let changed =
        canvas.apply_target_area_to_selected_images(crate::jobs::DEFAULT_IMPORT_TARGET_CANVAS_AREA);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Applied default average area to {changed} {noun}");
}

fn apply_average_selection_area(canvas: &mut crate::canvas::CanvasState, status: &mut String) {
    let Some(target_area) = canvas.average_selected_image_area() else {
        *status = "Select 2 or more images to average their area".to_owned();
        return;
    };
    let changed = canvas.apply_target_area_to_selected_images(target_area);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Averaged area across {changed} {noun}");
}

fn block_sidebar_input(ctx: &egui::Context, sidebar_rect: egui::Rect) {
    if sidebar_rect.height() <= 0.0 || sidebar_rect.width() <= 0.0 {
        return;
    }

    egui::Area::new(egui::Id::new("sidebar_blocker"))
        .order(egui::Order::Foreground)
        .fixed_pos(sidebar_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(sidebar_rect.size());
            ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
        });
}

fn close_sidebar_on_outside_click(
    ctx: &egui::Context,
    sidebar_rect: egui::Rect,
    sidebar_open_state: &mut bool,
) {
    let clicked_outside = ctx.input(|input| {
        should_close_sidebar(
            input.pointer.any_click(),
            input.pointer.interact_pos(),
            sidebar_rect,
        )
    });

    if clicked_outside {
        *sidebar_open_state = false;
    }
}

fn should_close_sidebar(
    pointer_clicked: bool,
    pointer_pos: Option<egui::Pos2>,
    sidebar_rect: egui::Rect,
) -> bool {
    pointer_clicked && pointer_pos.is_some_and(|pos| !sidebar_rect.contains(pos))
}

#[cfg(feature = "semantic-search")]
#[derive(Clone, Copy)]
struct SearchResultGridLayout {
    columns: usize,
    tile_width: f32,
}

#[cfg(feature = "semantic-search")]
fn search_overlay_max_height(screen_rect: egui::Rect) -> f32 {
    (screen_rect.height() * SEARCH_OVERLAY_MAX_SCREEN_HEIGHT_RATIO)
        .min(screen_rect.height() - SEARCH_OVERLAY_TOP_MARGIN - SEARCH_OVERLAY_BOTTOM_MARGIN)
}

#[cfg(feature = "semantic-search")]
fn search_result_tile_height() -> f32 {
    SEARCH_RESULT_TILE_PREVIEW_HEIGHT + SEARCH_RESULT_TILE_FOOTER_HEIGHT
}

#[cfg(feature = "semantic-search")]
fn search_overlay_width(_screen_rect: egui::Rect, _result_count: usize) -> f32 {
    PALETTE_WIDTH
}

#[cfg(feature = "semantic-search")]
fn search_result_grid_layout(available_width: f32, result_count: usize) -> SearchResultGridLayout {
    let gap = crate::theme::SPACE_2;
    let columns = PALETTE_GRID_COLUMNS.min(result_count.max(1));
    let tile_width = (available_width - gap * (columns.saturating_sub(1) as f32)) / columns as f32;
    SearchResultGridLayout {
        columns,
        tile_width,
    }
}

#[cfg(feature = "semantic-search")]
fn search_result_grid_content_height(result_count: usize, columns: usize) -> f32 {
    if result_count == 0 {
        return 0.0;
    }

    let rows = result_count.div_ceil(columns.max(1));
    rows as f32 * search_result_tile_height()
        + (rows.saturating_sub(1) as f32) * crate::theme::SPACE_2
}

#[cfg(feature = "semantic-search")]
fn search_result_tile(
    ui: &mut egui::Ui,
    canvas: &crate::canvas::CanvasState,
    group: &SearchResultGroup,
    active: bool,
    tile_width: f32,
) -> egui::Response {
    let tile_size = egui::vec2(
        tile_width,
        SEARCH_RESULT_TILE_PREVIEW_HEIGHT + SEARCH_RESULT_TILE_FOOTER_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(tile_size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    let tile_radius = if active { 0 } else { crate::theme::CARD_RADIUS };

    ui.painter()
        .rect_filled(rect, tile_radius, crate::theme::SURFACE);
    if active {
        ui.painter().rect_stroke(
            rect,
            tile_radius,
            crate::theme::stroke_strong(),
            egui::StrokeKind::Outside,
        );
    }

    let preview_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x, rect.min.y + SEARCH_RESULT_TILE_PREVIEW_HEIGHT),
    );

    if let Some(item_id) = group.item_ids.first().copied()
        && let Some(preview) = canvas.preview_for_item_id(item_id)
    {
        let preview_aspect = preview_rect.width() / preview_rect.height().max(1.0);
        let texture_aspect = preview.texture_size.x / preview.texture_size.y.max(1.0);
        let uv_rect = if texture_aspect > preview_aspect {
            let visible_u = preview_aspect / texture_aspect;
            let inset = (1.0 - visible_u) * 0.5;
            egui::Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
        } else {
            let visible_v = texture_aspect / preview_aspect;
            let inset = (1.0 - visible_v) * 0.5;
            egui::Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
        };
        ui.painter().image(
            preview.texture_id,
            preview_rect,
            uv_rect,
            egui::Color32::WHITE,
        );
    } else {
        ui.painter()
            .rect_filled(preview_rect, 0, crate::theme::BACKGROUND);
    }

    let primary_color = if active || hovered {
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let metadata_color = if active || hovered {
        crate::theme::TEXT_MUTED
    } else {
        crate::theme::TEXT_SUBTLE
    };

    let primary_text = format!("#{}", group.asset_id);
    let metadata_text = format!("{}x  ·  {:.2}", group.instance_count, group.similarity);
    let primary_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let metadata_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let primary_galley = ui
        .painter()
        .layout_no_wrap(primary_text, primary_font, primary_color);
    let metadata_galley = ui
        .painter()
        .layout_no_wrap(metadata_text, metadata_font, metadata_color);

    let content_rect = egui::Rect::from_min_max(
        egui::pos2(
            rect.left() + crate::theme::SPACE_2,
            preview_rect.bottom() + crate::theme::SPACE_1,
        ),
        egui::pos2(
            rect.right() - crate::theme::SPACE_2,
            rect.bottom() - crate::theme::SPACE_1,
        ),
    );
    let primary_pos = egui::pos2(
        content_rect.left(),
        content_rect.center().y - primary_galley.size().y * 0.5,
    );
    let metadata_pos = egui::pos2(
        content_rect.right() - metadata_galley.size().x,
        content_rect.center().y - metadata_galley.size().y * 0.5,
    );

    ui.painter()
        .galley(primary_pos, primary_galley, primary_color);
    ui.painter()
        .galley(metadata_pos, metadata_galley, metadata_color);

    response
}

#[cfg(feature = "semantic-search")]
fn paint_palette_input_row(
    ui: &mut egui::Ui,
    query: &mut String,
    input_id: egui::Id,
) -> egui::Response {
    let mut response = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (icon_rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                paint_search_icon(ui, icon_rect, crate::theme::TEXT_SUBTLE);

                let mut visuals = ui.style_mut().visuals.clone();
                visuals.extreme_bg_color = egui::Color32::TRANSPARENT;
                visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                visuals.widgets.active.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                visuals.widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                visuals.selection.bg_fill = crate::theme::SURFACE_ELEVATED;
                ui.style_mut().visuals = visuals;

                let resp = ui.add_sized(
                    egui::vec2(ui.available_width() - 44.0, 24.0),
                    egui::TextEdit::singleline(query)
                        .id(input_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("Search images, tags, boards")
                        .frame(egui::Frame::NONE)
                        .margin(egui::vec2(0.0, 0.0)),
                );
                paint_kbd_pill(ui, "esc");
                response = Some(resp);
            });
        });
    response.expect("palette input row allocated")
}

#[cfg(feature = "semantic-search")]
fn paint_search_icon(ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let painter = ui.painter();
    let center = rect.center();
    let r = rect.width() * 0.28;
    let stroke = egui::Stroke::new(1.4, color);
    painter.circle_stroke(center - egui::vec2(r * 0.3, r * 0.3), r, stroke);
    painter.line_segment(
        [
            center + egui::vec2(r * 0.4, r * 0.4),
            center + egui::vec2(r * 1.1, r * 1.1),
        ],
        stroke,
    );
}

#[cfg(feature = "semantic-search")]
fn paint_kbd_pill(ui: &mut egui::Ui, label: &str) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(15))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(15)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 5,
            right: 5,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Monospace)
                    .color(crate::theme::TEXT_MUTED)
                    .size(10.0),
            );
        });
}

#[cfg(feature = "semantic-search")]
fn paint_palette_facet_row(ui: &mut egui::Ui, result_count: usize) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 0,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new("filter")
                        .text_style(egui::TextStyle::Monospace)
                        .color(crate::theme::TEXT_SUBTLE)
                        .size(10.0),
                );
                paint_facet_btn(ui, "Color", Some(crate::theme::ACCENT));
                paint_facet_btn(ui, "Kind", None);
                paint_facet_btn(ui, "Orientation", None);
                paint_facet_btn(ui, "Source", None);
                paint_facet_btn(ui, "Visual", None);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{result_count}"))
                            .text_style(egui::TextStyle::Monospace)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(10.0),
                    );
                });
            });
        });
}

#[cfg(feature = "semantic-search")]
fn paint_facet_btn(ui: &mut egui::Ui, label: &str, dot: Option<egui::Color32>) -> egui::Response {
    let label_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), label_font, crate::theme::TEXT_MUTED);
    let mut content_w = label_galley.size().x + 4.0 + 8.0;
    if dot.is_some() {
        content_w += 8.0 + 4.0;
    }
    let pad_x = 8.0_f32;
    let btn_size = egui::vec2(content_w + pad_x * 2.0 - 4.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(btn_size, egui::Sense::click());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(4),
        egui::Color32::from_white_alpha(9),
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(15)),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + pad_x;
    let center_y = rect.center().y;
    if let Some(dot_color) = dot {
        ui.painter()
            .circle_filled(egui::pos2(x + 4.0, center_y), 4.0, dot_color);
        x += 12.0;
    }
    ui.painter().galley(
        egui::pos2(x, center_y - label_galley.size().y * 0.5),
        label_galley,
        crate::theme::TEXT_MUTED,
    );
    let chev_x = rect.right() - pad_x + 2.0;
    let chev_stroke = egui::Stroke::new(1.2, crate::theme::TEXT_SUBTLE);
    ui.painter().line_segment(
        [
            egui::pos2(chev_x - 4.0, center_y - 2.0),
            egui::pos2(chev_x - 1.0, center_y + 1.0),
        ],
        chev_stroke,
    );
    ui.painter().line_segment(
        [
            egui::pos2(chev_x - 1.0, center_y + 1.0),
            egui::pos2(chev_x + 2.0, center_y - 2.0),
        ],
        chev_stroke,
    );
    response
}

#[cfg(feature = "semantic-search")]
fn paint_palette_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0, egui::Color32::from_white_alpha(14));
}

#[cfg(feature = "semantic-search")]
fn paint_palette_empty_state(ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 12,
            bottom: 6,
        })
        .show(ui, |ui| {
            paint_palette_empty_group(ui, "RECENT", &["boat", "forest path", "still life"]);
            paint_palette_empty_group(ui, "SAVED SEARCHES", &["Warm tones", "Cold tones"]);
            paint_palette_empty_group(ui, "BOARDS", &["All images"]);
        });
}

#[cfg(feature = "semantic-search")]
fn paint_palette_empty_group(ui: &mut egui::Ui, title: &str, rows: &[&str]) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .size(9.0),
            );
        });
    for row in rows {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 6,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(*row)
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT),
                );
            });
    }
    ui.add_space(4.0);
}

#[cfg(feature = "semantic-search")]
fn paint_palette_footer(ui: &mut egui::Ui, result_count: usize, status: &SearchOverlayStatus) {
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(38))
        .inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 9,
            bottom: 9,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if result_count > 0 {
                    paint_create_board_pill(ui, result_count);
                    ui.label(
                        egui::RichText::new("or drag individual images")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                } else {
                    let idle_text = match status {
                        SearchOverlayStatus::Idle => "Ready".to_owned(),
                        SearchOverlayStatus::Indexing => "Indexing…".to_owned(),
                        SearchOverlayStatus::Loading(msg) => msg.clone(),
                        SearchOverlayStatus::Ready => "Ready".to_owned(),
                        SearchOverlayStatus::Error(err) => err.clone(),
                    };
                    let dot_rect = ui
                        .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                        .0;
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        3.0,
                        crate::theme::INDEXED_DOT,
                    );
                    ui.label(
                        egui::RichText::new(idle_text)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new("board")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                    paint_kbd_pill(ui, "↵");
                    paint_kbd_pill(ui, "⌘");
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("open")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                    paint_kbd_pill(ui, "↵");
                });
            });
        });
}

#[cfg(feature = "semantic-search")]
fn paint_create_board_pill(ui: &mut egui::Ui, result_count: usize) {
    let accent = crate::theme::ACCENT;
    let bg = egui::Color32::from_rgba_unmultiplied(228, 206, 176, 31);
    let stroke = egui::Stroke::new(
        1.0,
        egui::Color32::from_rgba_unmultiplied(228, 206, 176, 89),
    );
    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin {
            left: 10,
            right: 10,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(egui::RichText::new("+").color(accent).strong().size(11.0));
                ui.label(
                    egui::RichText::new(format!("Create board from {result_count} results"))
                        .color(accent)
                        .strong()
                        .size(11.0),
                );
            });
        });
}

struct SidebarRenderResult {
    settings_changed: bool,
    interactive_rect: Option<egui::Rect>,
}

fn show_sidebar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
    sidebar_tab: &mut SidebarTab,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> SidebarRenderResult {
    if !sidebar_open {
        return SidebarRenderResult {
            settings_changed: false,
            interactive_rect: None,
        };
    }

    let mut settings_changed = false;

    let top = screen_rect.top() + SIDEBAR_TOP_MARGIN;
    let bottom = (screen_rect.bottom() - SIDEBAR_BOTTOM_MARGIN).max(top);
    let sidebar_rect = egui::Rect::from_min_max(
        egui::pos2(screen_rect.left(), top),
        egui::pos2(screen_rect.left() + SIDEBAR_WIDTH, bottom),
    );

    let area_response = egui::Area::new(egui::Id::new("sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(sidebar_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(sidebar_rect.size());
            let shape_rect = sidebar_rect;
            let radius = egui::CornerRadius {
                nw: 0,
                ne: crate::theme::CARD_RADIUS,
                sw: 0,
                se: crate::theme::CARD_RADIUS,
            };
            ui.painter().rect(
                shape_rect,
                radius,
                crate::theme::SURFACE,
                crate::theme::stroke_subtle(),
                egui::StrokeKind::Inside,
            );

            let inner_rect = shape_rect.shrink2(egui::vec2(0.0, 0.0));
            let ui_builder = egui::UiBuilder::new().max_rect(inner_rect);
            ui.scope_builder(ui_builder, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                paint_sidebar_header(ui);
                paint_sidebar_tabs(ui, sidebar_tab);
                ui.add_space(crate::theme::SPACE_2);
                match *sidebar_tab {
                    SidebarTab::Boards => paint_sidebar_boards(ui, canvas),
                    SidebarTab::Options => {
                        settings_changed |= paint_sidebar_options(
                            ui,
                            auto_image_scaling,
                            import_gap,
                            canvas_image_mode,
                            canvas_image_max_side,
                            semantic_search_enabled,
                            canvas,
                        );
                    }
                }
            });
        });

    SidebarRenderResult {
        settings_changed,
        interactive_rect: Some(area_response.response.rect),
    }
}

fn paint_sidebar_header(ui: &mut egui::Ui) {
    let rect = ui
        .allocate_space(egui::vec2(
            ui.available_width(),
            crate::theme::CONTROL_HEIGHT + crate::theme::SPACE_2,
        ))
        .1;
    let pad_x = crate::theme::SPACE_4;
    let center_y = rect.center().y;
    let badge_size = 22.0;
    let badge_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + pad_x + badge_size * 0.5, center_y),
        egui::vec2(badge_size, badge_size),
    );
    ui.painter().rect_filled(
        badge_rect,
        egui::CornerRadius::same(5),
        crate::theme::BADGE,
    );
    let badge_text_color = egui::Color32::from_rgb(0x1A, 0x19, 0x17);
    let badge_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let badge_galley = ui
        .painter()
        .layout_no_wrap("R".to_owned(), badge_font, badge_text_color);
    let badge_pos = egui::pos2(
        badge_rect.center().x - badge_galley.size().x * 0.5,
        badge_rect.center().y - badge_galley.size().y * 0.5,
    );
    ui.painter()
        .galley(badge_pos, badge_galley, badge_text_color);

    let name_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let name_galley = ui.painter().layout_no_wrap(
        "Reference".to_owned(),
        name_font,
        crate::theme::TEXT,
    );
    let name_pos = egui::pos2(
        badge_rect.right() + crate::theme::SPACE_2,
        center_y - name_galley.size().y * 0.5,
    );
    ui.painter().galley(name_pos, name_galley, crate::theme::TEXT);

    let kbd_label = sidebar_shortcut_label();
    let kbd_font = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    let kbd_galley =
        ui.painter()
            .layout_no_wrap(kbd_label.to_owned(), kbd_font, crate::theme::TEXT_SUBTLE);
    let kbd_pad = egui::vec2(crate::theme::SPACE_1 + 1.0, 2.0);
    let kbd_size = kbd_galley.size() + kbd_pad * 2.0;
    let kbd_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - pad_x - kbd_size.x, center_y - kbd_size.y * 0.5),
        kbd_size,
    );
    ui.painter().rect(
        kbd_rect,
        egui::CornerRadius::same(4),
        crate::theme::SURFACE_ELEVATED,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        kbd_rect.min + kbd_pad,
        kbd_galley,
        crate::theme::TEXT_SUBTLE,
    );
}

fn paint_sidebar_tabs(ui: &mut egui::Ui, sidebar_tab: &mut SidebarTab) {
    let bar_height = 32.0;
    let (_id, rect) =
        ui.allocate_space(egui::vec2(ui.available_width(), bar_height + 1.0));
    let pad_x = crate::theme::SPACE_3;
    let tab_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left() + pad_x, rect.top()), rect.max);
    let tabs: [(&str, SidebarTab); 2] = [
        ("Boards", SidebarTab::Boards),
        ("Options", SidebarTab::Options),
    ];
    let tab_width = (tab_rect.width() - pad_x) / tabs.len() as f32;
    for (index, (label, tab_value)) in tabs.iter().enumerate() {
        let cell_min = egui::pos2(tab_rect.left() + tab_width * index as f32, tab_rect.top());
        let cell = egui::Rect::from_min_size(cell_min, egui::vec2(tab_width, bar_height));
        let selected = *sidebar_tab == *tab_value;
        let response = ui.interact(
            cell,
            egui::Id::new(("sidebar_tab", label)),
            egui::Sense::click(),
        );
        if response.clicked() {
            *sidebar_tab = *tab_value;
        }
        let label_color = if selected {
            crate::theme::TEXT
        } else if response.hovered() {
            crate::theme::TEXT_MUTED
        } else {
            crate::theme::TEXT_SUBTLE
        };
        let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
        let galley = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), font, label_color);
        let label_pos = egui::pos2(
            cell.center().x - galley.size().x * 0.5,
            cell.center().y - galley.size().y * 0.5 - 2.0,
        );
        ui.painter().galley(label_pos, galley, label_color);
        if selected {
            let underline_y = cell.bottom();
            let underline = egui::Rect::from_min_max(
                egui::pos2(cell.center().x - 18.0, underline_y - 1.5),
                egui::pos2(cell.center().x + 18.0, underline_y),
            );
            ui.painter()
                .rect_filled(underline, 0, crate::theme::ACCENT);
        }
    }
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.bottom(),
        crate::theme::stroke_subtle(),
    );
}

fn paint_sidebar_options(
    ui: &mut egui::Ui,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_2);
        egui::ScrollArea::vertical()
            .id_salt("sidebar_options_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_3);
                ui.add_space(crate::theme::SPACE_2);
                paint_sidebar_section_header(ui, "Options", Some("Board defaults"));
                paint_sidebar_subcard(ui, |ui| {
                    changed |= paint_settings_section(
                        ui,
                        auto_image_scaling,
                        import_gap,
                        canvas_image_mode,
                        canvas_image_max_side,
                        semantic_search_enabled,
                        canvas,
                    );
                });
                ui.add_space(crate::theme::SPACE_4);
                paint_sidebar_section_header(ui, "Shortcuts", None);
                paint_supported_shortcuts_section(ui);
                ui.add_space(crate::theme::SPACE_4);
            });
    });
    changed
}

fn paint_sidebar_boards(ui: &mut egui::Ui, _canvas: &mut crate::canvas::CanvasState) {
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("sidebar_boards_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);
                ui.add_space(crate::theme::SPACE_2);
                let indent = crate::theme::SPACE_2;
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        egui::pos2(ui.max_rect().left() + indent, ui.max_rect().top()),
                        egui::pos2(ui.max_rect().right() - indent, ui.max_rect().bottom()),
                    )),
                    |ui| {
                        paint_board_row(
                            ui,
                            "Current board",
                            "All images",
                            true,
                        );
                        ui.add_space(crate::theme::SPACE_2);
                        paint_create_board_button(ui);
                        ui.add_space(crate::theme::SPACE_3);
                        ui.label(
                            egui::RichText::new("Boards help you split your canvas into focused collections.")
                                .text_style(egui::TextStyle::Small)
                                .color(crate::theme::TEXT_SUBTLE),
                        );
                    },
                );
            });
    });
}

fn paint_board_row(ui: &mut egui::Ui, name: &str, detail: &str, active: bool) {
    let height = 36.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let fill = if active {
        crate::theme::SURFACE_ELEVATED
    } else if response.hovered() {
        crate::theme::SURFACE_ELEVATED
    } else {
        crate::theme::SURFACE
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), fill);

    let swatch_size = 22.0;
    let swatch_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + crate::theme::SPACE_2,
            rect.center().y - swatch_size * 0.5,
        ),
        egui::vec2(swatch_size, swatch_size),
    );
    ui.painter().rect_filled(
        swatch_rect,
        egui::CornerRadius::same(3),
        crate::theme::BORDER,
    );

    let name_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let name_galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), name_font, crate::theme::TEXT);
    let detail_galley = ui.painter().layout_no_wrap(
        detail.to_owned(),
        detail_font,
        crate::theme::TEXT_SUBTLE,
    );

    let text_x = swatch_rect.right() + crate::theme::SPACE_2;
    let name_pos = egui::pos2(text_x, rect.center().y - name_galley.size().y - 1.0);
    let detail_pos = egui::pos2(text_x, rect.center().y + 1.0);
    ui.painter().galley(name_pos, name_galley, crate::theme::TEXT);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::theme::TEXT_SUBTLE);

    if active {
        let dot = egui::Rect::from_center_size(
            egui::pos2(rect.right() - crate::theme::SPACE_3, rect.center().y),
            egui::vec2(5.0, 5.0),
        );
        ui.painter()
            .rect_filled(dot, egui::CornerRadius::same(3), crate::theme::ACCENT);
    }
}

fn paint_create_board_button(ui: &mut egui::Ui) {
    let height = 30.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let dash_stroke = egui::Stroke::new(1.0, crate::theme::BORDER);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(5),
        if response.hovered() {
            crate::theme::SURFACE_ELEVATED
        } else {
            egui::Color32::TRANSPARENT
        },
        dash_stroke,
        egui::StrokeKind::Inside,
    );

    let plus_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let plus_galley = ui.painter().layout_no_wrap(
        "+  Create new board".to_owned(),
        plus_font,
        crate::theme::TEXT_SUBTLE,
    );
    let pos = egui::pos2(
        rect.center().x - plus_galley.size().x * 0.5,
        rect.center().y - plus_galley.size().y * 0.5 - 1.0,
    );
    ui.painter()
        .galley(pos, plus_galley, crate::theme::TEXT_SUBTLE);
}

fn paint_sidebar_section_header(ui: &mut egui::Ui, title: &str, detail: Option<&str>) {
    let pad = crate::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.cursor().top() + 22.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
            ui.label(
                egui::RichText::new(title)
                    .text_style(crate::theme::display_text_style())
                    .color(crate::theme::TEXT),
            );
            if let Some(detail) = detail {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(detail)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE),
                    );
                });
            }
        });
    });
    ui.add_space(22.0);
}

fn paint_sidebar_subcard(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let pad = crate::theme::SPACE_2;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from_white_alpha(5))
            .stroke(crate::theme::stroke_subtle())
            .corner_radius(egui::CornerRadius::same(7))
            .inner_margin(egui::Margin {
                left: crate::theme::SPACE_3 as i8,
                right: crate::theme::SPACE_3 as i8,
                top: crate::theme::SPACE_2 as i8,
                bottom: crate::theme::SPACE_2 as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing =
                    egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_1);
                add_contents(ui);
            });
    });
}

fn paint_settings_section(
    ui: &mut egui::Ui,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);

        paint_sidebar_subsection_label(ui, "Canvas");
        ui.add_space(crate::theme::SPACE_1);

        changed |= paint_setting_row(ui, "Canvas background", "Quiet board guide.", |ui| {
            paint_background_mode_control(ui, canvas)
        });
        changed |= paint_setting_row(
            ui,
            "Canvas image resolution",
            "Keeps originals for crop/edit.",
            |ui| paint_canvas_image_mode_control(ui, canvas_image_mode),
        );
        changed |= paint_setting_row(
            ui,
            "Canvas max dimension",
            "Largest side of the canvas copy.",
            |ui| {
                ui.add_enabled_ui(
                    *canvas_image_mode == crate::data::CanvasImageMode::Downscale,
                    |ui| {
                        ui.add(
                            egui::DragValue::new(canvas_image_max_side)
                                .speed(64.0)
                                .range(256..=8192)
                                .fixed_decimals(0)
                                .suffix(" px"),
                        )
                        .changed()
                    },
                )
                .inner
            },
        );

        ui.add_space(crate::theme::SPACE_1);
        paint_sidebar_subsection_label(ui, "Import + search");
        ui.add_space(crate::theme::SPACE_1);

        let auto_scale_detail = "Caps new imports to a default area.";
        changed |= paint_setting_row(ui, "Auto-scale new images", auto_scale_detail, |ui| {
            #[cfg(feature = "semantic-search")]
            {
                paint_switch(ui, auto_image_scaling).changed()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                ui.add_enabled(
                    false,
                    egui::widgets::Checkbox::without_text(auto_image_scaling),
                )
                .changed()
            }
        });
        changed |= paint_setting_row(
            ui,
            "Import gap",
            "Gap used when placing new images.",
            |ui| {
                ui.add(
                    egui::DragValue::new(import_gap)
                        .speed(0.5)
                        .range(0.0..=128.0)
                        .fixed_decimals(0)
                        .suffix(" units"),
                )
                .changed()
            },
        );
        changed |= paint_setting_row(
            ui,
            "Semantic search",
            #[cfg(feature = "semantic-search")]
            "Experimental; downloads a model on first use.",
            #[cfg(not(feature = "semantic-search"))]
            "Feature disabled in this build.",
            |ui| {
                #[cfg(feature = "semantic-search")]
                {
                    paint_switch(ui, semantic_search_enabled).changed()
                }
                #[cfg(not(feature = "semantic-search"))]
                {
                    ui.add_enabled(
                        false,
                        egui::widgets::Checkbox::without_text(semantic_search_enabled),
                    )
                    .changed()
                }
            },
        );
    });

    changed
}

fn paint_setting_row(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Body)
                .strong()
                .color(crate::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(detail)
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
        );
        ui.add_space(crate::theme::SPACE_1);
        changed |= add_control(ui);
        ui.add_space(crate::theme::SPACE_2);
    });
    changed
}

fn paint_canvas_image_mode_control(
    ui: &mut egui::Ui,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .radio_value(
                canvas_image_mode,
                crate::data::CanvasImageMode::Downscale,
                "Downscale",
            )
            .changed();
        changed |= ui
            .radio_value(
                canvas_image_mode,
                crate::data::CanvasImageMode::Original,
                "Original",
            )
            .changed();
    });
    changed
}

fn paint_background_mode_control(
    ui: &mut egui::Ui,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut background = canvas.background();
    let before = background;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_1;
        for (label, value) in [
            ("Dots", crate::data::CanvasBackground::Dots),
            ("Grid", crate::data::CanvasBackground::Grid),
            ("None", crate::data::CanvasBackground::None),
        ] {
            let selected = background == value;
            let button = egui::Button::new(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Button)
                    .color(crate::theme::TEXT),
            )
            .fill(if selected {
                crate::theme::SURFACE_ELEVATED
            } else {
                crate::theme::SURFACE
            })
            .stroke(if selected {
                crate::theme::stroke_strong()
            } else {
                crate::theme::stroke_subtle()
            })
            .corner_radius(crate::theme::CONTROL_RADIUS)
            .min_size(egui::vec2(44.0, crate::theme::CONTROL_HEIGHT));

            if ui.add(button).clicked() {
                background = value;
            }
        }
    });

    if background != before {
        canvas.set_background(background);
        true
    } else {
        false
    }
}

fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    paint_sidebar_subsection_label(ui, "Implemented");
    ui.add_space(crate::theme::SPACE_1);
    ui.label(
        egui::RichText::new("Bindings grouped by task.")
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_MUTED),
    );
    ui.add_space(crate::theme::SPACE_2);
    let (core_shortcuts, layout_shortcuts) = SUPPORTED_SHORTCUTS.split_at(8);

    ui.columns(2, |columns| {
        paint_shortcut_table_section(&mut columns[0], "Core actions", core_shortcuts);
        paint_shortcut_table_section(&mut columns[1], "Layout + history", layout_shortcuts);
    });
}

fn paint_shortcut_table_section(ui: &mut egui::Ui, title: &str, shortcuts: &[SupportedShortcut]) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);
        paint_sidebar_subsection_label(ui, title);
        ui.add_space(crate::theme::SPACE_1);
        paint_shortcut_grid(
            ui,
            egui::Id::new(("shortcut_section_grid", title)),
            shortcuts.iter().copied().map(shortcut_to_row),
        );
    });
}

fn shortcut_to_row(shortcut: SupportedShortcut) -> ShortcutRow {
    ShortcutRow {
        action: shortcut.action,
        windows: shortcut.windows,
        mac: shortcut.mac,
    }
}

fn paint_switch(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let desired_size = egui::vec2(40.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }

    let track_color = if *value {
        crate::theme::ACCENT
    } else {
        crate::theme::SURFACE_ELEVATED
    };
    let knob_color = if *value {
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let animation = ui.ctx().animate_bool(response.id, *value);
    let knob_radius = 9.0;
    let knob_x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), animation);
    let track_rect = rect.shrink2(egui::vec2(1.0, 3.0));
    let stroke_color = if response.hovered() {
        crate::theme::BORDER_STRONG
    } else {
        crate::theme::BORDER
    };

    ui.painter().rect(
        track_rect,
        track_rect.height() * 0.5,
        track_color,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Middle,
    );
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), knob_radius, knob_color);

    response
}

fn show_sidebar_hint(ctx: &egui::Context, screen_rect: egui::Rect, sidebar_open: bool) {
    if sidebar_open {
        return;
    }

    egui::Area::new(egui::Id::new("sidebar_hint"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.right() - crate::theme::SPACE_4,
            screen_rect.bottom() - crate::theme::SPACE_4,
        ))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            egui::vec2(-crate::theme::SPACE_4, -crate::theme::SPACE_4),
        )
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::SURFACE_ELEVATED)
                .stroke(crate::theme::stroke_subtle())
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 6,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(sidebar_shortcut_label())
                            .text_style(egui::TextStyle::Monospace)
                            .color(crate::theme::TEXT_MUTED),
                    );
                });
        });
}

fn paint_sidebar_subsection_label(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .text_style(egui::TextStyle::Small)
            .strong()
            .color(crate::theme::TEXT_SUBTLE),
    );
}

fn paint_shortcut_grid(
    ui: &mut egui::Ui,
    id: egui::Id,
    rows: impl IntoIterator<Item = ShortcutRow>,
) {
    egui::Grid::new(id)
        .num_columns(3)
        .striped(false)
        .spacing(egui::vec2(crate::theme::SPACE_1, 2.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Action")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("Win/Linux")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("Mac")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.end_row();

            for row in rows {
                ui.label(
                    egui::RichText::new(row.action)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT),
                );
                paint_shortcut_keys(ui, row.windows);
                paint_shortcut_keys(ui, row.mac);
                ui.end_row();
            }
        });
}

fn paint_shortcut_keys(ui: &mut egui::Ui, shortcut: &str) {
    if shortcut == "None" {
        ui.label(
            egui::RichText::new("None")
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_SUBTLE),
        );
        return;
    }

    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(crate::theme::CONTROL_RADIUS.saturating_sub(2))
        .inner_margin(egui::Margin {
            left: 5,
            right: 5,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(shortcut)
                    .text_style(egui::TextStyle::Monospace)
                    .color(crate::theme::TEXT)
                    .strong(),
            );
        });
}

fn sidebar_shortcut_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "⌘B"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+B"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArrangementAction, ReferenceApp, SEARCH_OVERLAY_EMPTY_MAX_WIDTH, apply_arrangement_action,
        search_overlay_width, search_result_grid_content_height, search_result_grid_layout,
        search_result_tile_height, should_close_sidebar,
    };
    use crate::data::{BoardItem, BoardItemWithAsset, ImageAsset};
    use crate::jobs::LoadedCanvasItem;
    use crate::storage::Library;
    use eframe::egui;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn persist_settings_if_changed_true_calls_save_once() {
        let mut calls = 0;
        ReferenceApp::persist_settings_if_changed_with(true, || {
            calls += 1;
        });
        assert_eq!(calls, 1);
    }

    #[test]
    fn persist_settings_if_changed_false_skips_save() {
        let mut calls = 0;
        ReferenceApp::persist_settings_if_changed_with(false, || {
            calls += 1;
        });
        assert_eq!(calls, 0);
    }

    #[test]
    fn save_user_session_round_trips_import_gap() {
        let root = temp_library_root("reference-app-gap");
        let library = Library::initialize(root.clone()).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.auto_image_scaling = false;
        app.import_gap = 37.0;
        app.canvas_image_mode = crate::data::CanvasImageMode::Original;
        app.canvas_image_max_side = 4096;
        app.canvas
            .set_background(crate::data::CanvasBackground::Grid);

        app.save_user_session();

        let session = library.load_user_session().unwrap();
        assert!(!session.auto_image_scaling);
        assert_eq!(session.import_gap, 37.0);
        assert_eq!(
            session.canvas_image_mode,
            crate::data::CanvasImageMode::Original
        );
        assert_eq!(session.canvas_image_max_side, 4096);
        assert_eq!(
            session.canvas_background,
            crate::data::CanvasBackground::Grid
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compact_arrangement_action_uses_dense_compact_layout() {
        let mut canvas = crate::canvas::CanvasState::default();
        let ctx = egui::Context::default();
        let fixture = [
            (1_i64, 300_u32, 520_u32),
            (2, 180, 260),
            (3, 160, 260),
            (4, 150, 220),
            (5, 150, 220),
            (6, 140, 180),
            (7, 140, 180),
        ];
        let gap = 12.0;

        for (index, (id, width, height)) in fixture.iter().enumerate() {
            let asset = ImageAsset {
                id: *id,
                original_path: PathBuf::from(format!("fixture-{id}.png")),
                filename: format!("fixture-{id}.png"),
                mime_type: "image/png".to_owned(),
                width: *width,
                height: *height,
                file_size: 0,
                imported_at: 0,
            };
            let record = BoardItemWithAsset {
                item: BoardItem {
                    id: *id,
                    asset_id: *id,
                    x: index as f32 * 500.0,
                    y: index as f32 * 120.0,
                    scale: 1.0,
                    rotation: 0.0,
                    z_index: index as i32,
                    archived: false,
                },
                asset,
            };
            let rgba = vec![255; (*width as usize) * (*height as usize) * 4];
            canvas.add_loaded_item(
                &ctx,
                LoadedCanvasItem {
                    queue_id: None,
                    record,
                    preview_rgba: rgba,
                    preview_width: *width,
                    preview_height: *height,
                    preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                    loaded_tier: crate::data::CanvasImageCacheTier::Working,
                    existing_load_diagnostics: None,
                    origin: crate::jobs::ImportOrigin::Existing,
                    trace: None,
                },
            );
        }

        canvas.select_items(&fixture.iter().map(|(id, _, _)| *id).collect::<Vec<_>>());
        let mut status = String::new();
        apply_arrangement_action(&mut canvas, ArrangementAction::Compact, gap, &mut status);

        let bounds = canvas.selected_bounds_world().unwrap();
        let arranged_area = bounds.width() * bounds.height();
        let legacy_area = crate::canvas::legacy_compact_layout_area_for_sizes(
            &fixture
                .iter()
                .map(|(id, width, height)| (*id, *width as f32, *height as f32))
                .collect::<Vec<_>>(),
            gap,
        )
        .unwrap();

        assert!(arranged_area < legacy_area);
        assert_eq!(status, format!("Compacted {} images", fixture.len()));
    }

    #[test]
    fn sidebar_close_helper_only_closes_for_outside_clicks() {
        let sidebar_rect =
            egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(220.0, 180.0));

        assert!(!should_close_sidebar(
            true,
            Some(egui::pos2(60.0, 90.0)),
            sidebar_rect
        ));
        assert!(should_close_sidebar(
            true,
            Some(egui::pos2(300.0, 90.0)),
            sidebar_rect
        ));
        assert!(!should_close_sidebar(
            false,
            Some(egui::pos2(300.0, 90.0)),
            sidebar_rect
        ));
        assert!(!should_close_sidebar(true, None, sidebar_rect));
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_shortcut_opens_overlay_when_enabled() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.semantic_search_enabled = true;

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::F,
                    physical_key: Some(egui::Key::F),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| app.handle_search_shortcut(ui.ctx()),
        );

        assert!(app.search_overlay.open);
        assert!(app.search_overlay.focus_requested);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn escape_closes_search_overlay_and_clears_matches() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.search_overlay.open = true;
        app.search_overlay.query = "cat".to_owned();
        app.canvas.set_search_matches(vec![1, 2], Some(1));

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: Some(egui::Key::Escape),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            },
            |ui| {
                app.show_search_overlay(
                    ui.ctx(),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
                )
            },
        );

        assert!(!app.search_overlay.open);
        assert!(app.search_overlay.query.is_empty());
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_overlay_focuses_text_input_when_requested() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.search_overlay.open = true;
        app.search_overlay.focus_requested = true;

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        });

        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("semantic_search_input"))));
        assert!(!app.search_overlay.focus_requested);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_overlay_width_stays_compact_without_results() {
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2048.0, 1280.0));

        let width = search_overlay_width(screen_rect, 0);

        assert_eq!(width, SEARCH_OVERLAY_EMPTY_MAX_WIDTH);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_result_grid_layout_balances_five_results_into_five_columns() {
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2048.0, 1280.0));
        let width = search_overlay_width(screen_rect, 5);
        let shell_content_width = width - 24.0;
        let grid_layout = search_result_grid_layout(shell_content_width, 5);

        assert_eq!(grid_layout.columns, 5);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_result_grid_content_height_grows_by_rows() {
        let height = search_result_grid_content_height(6, 5);

        assert_eq!(
            height,
            search_result_tile_height() * 2.0 + crate::theme::SPACE_2
        );
    }

    #[test]
    fn queued_import_placeholders_replace_and_cleanup_by_queue_id() {
        let mut app = test_app(None);

        app.queue_import_placeholder(7, "first".to_owned(), egui::pos2(10.0, 20.0), 300, 200);
        app.queue_import_placeholder(7, "second".to_owned(), egui::pos2(30.0, 40.0), 120, 90);

        assert_eq!(app.canvas.import_placeholder_count(), 1);
        app.canvas.remove_import_placeholder(7);
        assert_eq!(app.canvas.import_placeholder_count(), 0);
    }

    fn test_app(library: Option<Library>) -> ReferenceApp {
        ReferenceApp {
            canvas: crate::canvas::CanvasState::default(),
            screenshot_capture: crate::screenshot::ScreenshotCapture::new(None),
            window_capture: None,
            wgpu_render_state: None,
            library,
            import_worker: None,
            crop_undo_stack: Vec::new(),
            max_texture_side: 8192,
            auto_image_scaling: true,
            import_gap: crate::data::DEFAULT_IMPORT_GAP,
            canvas_image_mode: crate::data::CanvasImageMode::default(),
            canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            semantic_search_enabled: false,
            #[cfg(feature = "semantic-search")]
            semantic_search: None,
            #[cfg(feature = "semantic-search")]
            search_overlay: super::SearchOverlayState::default(),
            frame_perf: super::FramePerfState::default(),
            pending_cache_backfills: std::collections::HashSet::new(),
            inflight_image_tier_loads: std::collections::HashMap::new(),
            sidebar_open: false,
            sidebar_tab: super::SidebarTab::default(),
            status: String::new(),
            perf_log: None,
            next_import_queue_id: 1,
            #[cfg(debug_assertions)]
            verification_search_query: None,
            #[cfg(debug_assertions)]
            sidebar_hot_preview: false,
        }
    }

    fn temp_library_root(prefix: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
