use eframe::egui;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[cfg(feature = "semantic-search")]
use crate::palette::ImageSearchPivot;
use crate::residency::ImageResidencyCoordinator;
#[cfg(feature = "semantic-search")]
use crate::search::SearchController;
use crate::sidebar::{
    SidebarTab, block_sidebar_input, close_sidebar_on_outside_click, show_sidebar,
    show_sidebar_hint, sidebar_rect,
};

const COPY_PAYLOAD_KIND: &str = "reference-board-items";
const MAX_CROP_UNDO_ENTRIES: usize = 32;
const STARTUP_VIEWPORT_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
const MAX_IMPORT_RESULTS_PER_FRAME: usize = 4;

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ExternalImportSource {
    Path(std::path::PathBuf),
    Url(String),
}

#[derive(Clone, Copy)]
struct ShortcutBinding {
    key: egui::Key,
    command: bool,
    shift: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct SupportedShortcut {
    pub action: &'static str,
    #[allow(dead_code)]
    pub windows: &'static str,
    #[allow(dead_code)]
    pub mac: &'static str,
    binding: Option<ShortcutBinding>,
    shortcut_action: Option<SupportedShortcutAction>,
}

pub(crate) const SUPPORTED_SHORTCUTS: &[SupportedShortcut] = &[
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

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerificationMode {
    OpenSidebar,
    OpenSidebarHot,
    OpenSidebarNarrow,
    OpenSidebarBoards,
    OpenPalette,
    OpenPaletteImagePivot,
    DrawArrow,
    EditTextbox,
    RotateImage,
    AnimatedGif,
    MoveSnapPreview,
    ImportPlaceholders,
    ImportLoaded,
    BackgroundGrid,
    BackgroundDots,
    BackgroundTransition,
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
            } else if cfg!(debug_assertions) && argument == "--verify-palette-image-pivot" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenPaletteImagePivot);
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
            } else if cfg!(debug_assertions) && argument == "--verify-rotate-image" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::RotateImage);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-gif" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::AnimatedGif);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-move-snap" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::MoveSnapPreview);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-import-placeholders" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::ImportPlaceholders);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-import-loaded" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::ImportLoaded);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-background-grid" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BackgroundGrid);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-background-dots" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BackgroundDots);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-background-transition" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BackgroundTransition);
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
        matches!(
            self.verification_mode,
            Some(VerificationMode::OpenPalette | VerificationMode::OpenPaletteImagePivot)
        )
    }

    #[cfg(debug_assertions)]
    fn open_palette_image_pivot_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::OpenPaletteImagePivot)
        )
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
    fn rotate_image_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::RotateImage))
    }

    #[cfg(debug_assertions)]
    fn verify_gif_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::AnimatedGif))
    }

    #[cfg(debug_assertions)]
    fn verify_move_snap_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::MoveSnapPreview)
        )
    }

    #[cfg(debug_assertions)]
    fn verify_import_placeholders_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::ImportPlaceholders)
        )
    }

    #[cfg(debug_assertions)]
    fn verify_import_loaded_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::ImportLoaded))
    }

    #[cfg(debug_assertions)]
    fn background_verification_mode(&self) -> Option<VerificationMode> {
        match self.verification_mode {
            Some(
                VerificationMode::BackgroundGrid
                | VerificationMode::BackgroundDots
                | VerificationMode::BackgroundTransition,
            ) => self.verification_mode,
            _ => None,
        }
    }

    #[cfg(debug_assertions)]
    fn skip_existing_items_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(
                VerificationMode::DrawArrow
                    | VerificationMode::EditTextbox
                    | VerificationMode::RotateImage
                    | VerificationMode::AnimatedGif
                    | VerificationMode::MoveSnapPreview
                    | VerificationMode::ImportPlaceholders
                    | VerificationMode::ImportLoaded
                    | VerificationMode::BackgroundGrid
                    | VerificationMode::BackgroundDots
                    | VerificationMode::BackgroundTransition
            )
        )
    }

    #[cfg(debug_assertions)]
    fn verification_search_query(&self) -> Option<&str> {
        self.verification_search_query.as_deref()
    }

    #[cfg(not(debug_assertions))]
    fn skip_existing_items_on_start(&self) -> bool {
        false
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
    fn open_palette_image_pivot_on_start(&self) -> bool {
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
    settings: crate::data::WorkspaceSettings,
    #[cfg(feature = "semantic-search")]
    search: SearchController,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    status: String,
    perf_log: Option<PerfLogState>,
    frame_perf: FramePerfState,
    residency: ImageResidencyCoordinator,
    next_import_queue_id: u64,
    hover_preview_dimensions: std::collections::HashMap<std::path::PathBuf, Option<(u32, u32)>>,
    #[cfg(debug_assertions)]
    verification_mode: Option<VerificationMode>,
    #[cfg(debug_assertions)]
    verification_search_query: Option<String>,
    #[cfg(debug_assertions)]
    sidebar_hot_preview: bool,
}

impl ReferenceApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        crate::native_drop::install_url_drop_bridge(cc);
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
                    settings: crate::data::WorkspaceSettings {
                        semantic_search_enabled: false,
                        ..Default::default()
                    },
                    #[cfg(feature = "semantic-search")]
                    search: SearchController::default(),
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
                    residency: ImageResidencyCoordinator::default(),
                    next_import_queue_id: 1,
                    hover_preview_dimensions: std::collections::HashMap::new(),
                    #[cfg(debug_assertions)]
                    verification_mode: config.verification_mode,
                    #[cfg(debug_assertions)]
                    verification_search_query: None,
                    #[cfg(debug_assertions)]
                    sidebar_hot_preview: config.open_sidebar_hot_on_start(),
                };
                app.load_user_session();
                #[cfg(feature = "semantic-search")]
                app.sync_semantic_search_service();
                if !config.skip_existing_items_on_start() {
                    app.enqueue_existing_items();
                }
                #[cfg(debug_assertions)]
                if config.draw_arrow_on_start() {
                    app.canvas
                        .apply_viewport_state(&crate::data::ViewportState::default());
                    app.canvas.add_annotation(crate::data::CanvasAnnotation {
                        id: -1,
                        kind: crate::data::CanvasAnnotationKind::Arrow,
                        start_x: -220.0,
                        start_y: -120.0,
                        end_x: 220.0,
                        end_y: 120.0,
                        text: String::new(),
                        text_size: crate::data::CanvasTextSize::default(),
                        stroke_width: crate::data::CanvasStrokeWidth::L,
                        color: crate::data::CanvasAnnotationColor::Coral,
                        background_enabled: false,
                        archived: false,
                    });
                    app.canvas.select_single_annotation(-1);
                    app.status = "Added debug arrow".to_owned();
                }
                #[cfg(debug_assertions)]
                if config.edit_textbox_on_start() {
                    app.canvas
                        .apply_viewport_state(&crate::data::ViewportState::default());
                    app.canvas.add_annotation(crate::data::CanvasAnnotation {
                        id: -2,
                        kind: crate::data::CanvasAnnotationKind::Textbox,
                        start_x: -160.0,
                        start_y: -80.0,
                        end_x: 160.0,
                        end_y: 80.0,
                        text: "debug textbox".to_owned(),
                        text_size: crate::data::CanvasTextSize::default(),
                        stroke_width: crate::data::CanvasStrokeWidth::default(),
                        color: crate::data::CanvasAnnotationColor::Sky,
                        background_enabled: true,
                        archived: false,
                    });
                    app.canvas.begin_textbox_edit(-2);
                    app.status = "Editing debug textbox".to_owned();
                }
                #[cfg(debug_assertions)]
                if config.rotate_image_on_start() {
                    app.seed_rotation_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_gif_on_start() {
                    app.seed_gif_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_move_snap_on_start() {
                    app.seed_move_snap_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_import_placeholders_on_start() {
                    app.seed_import_verification_placeholders(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_import_loaded_on_start() {
                    app.seed_import_verification_loaded(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if let Some(mode) = config.background_verification_mode() {
                    app.seed_background_verification(mode);
                }
                #[cfg(debug_assertions)]
                if let Some(query) = config.verification_search_query().map(ToOwned::to_owned) {
                    app.settings.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    app.sync_semantic_search_service();
                    #[cfg(feature = "semantic-search")]
                    {
                        app.search.overlay.open = true;
                        app.search.overlay.focus_requested = true;
                        app.search.overlay.query = query;
                        app.search.overlay.last_query_edit_at = Some(Instant::now());
                    }
                    app.verification_search_query =
                        config.verification_search_query().map(ToOwned::to_owned);
                }
                #[cfg(debug_assertions)]
                if app.sidebar_hot_preview {
                    app.settings.auto_image_scaling = false;
                    app.settings.import_gap = 32.0;
                    app.settings.canvas_image_mode = crate::data::CanvasImageMode::Original;
                    app.settings.canvas_image_max_side = crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE;
                    app.canvas
                        .set_background(crate::data::CanvasBackground::Grid);
                    app.settings.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    app.sync_semantic_search_service();
                }
                #[cfg(debug_assertions)]
                if config.open_palette_on_start() {
                    app.settings.semantic_search_enabled = true;
                    #[cfg(feature = "semantic-search")]
                    {
                        app.sync_semantic_search_service();
                        app.search.overlay.open = true;
                        app.search.overlay.focus_requested = true;
                    }
                }
                #[cfg(debug_assertions)]
                if config.open_palette_image_pivot_on_start() {
                    #[cfg(feature = "semantic-search")]
                    {
                        app.search.overlay.image_pivot = Some(ImageSearchPivot {
                            asset_id: 42,
                            item_id: -1,
                        });
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
                settings: crate::data::WorkspaceSettings {
                    semantic_search_enabled: false,
                    ..Default::default()
                },
                #[cfg(feature = "semantic-search")]
                search: SearchController::default(),
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
                residency: ImageResidencyCoordinator::default(),
                next_import_queue_id: 1,
                hover_preview_dimensions: std::collections::HashMap::new(),
                #[cfg(debug_assertions)]
                verification_mode: config.verification_mode,
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
                                mode: self.settings.canvas_image_mode,
                                max_side: self.settings.canvas_image_max_side,
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
                self.canvas.apply_viewport_state(&session.viewport);
                self.settings = session.settings;
                self.canvas
                    .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
                self.canvas.set_layout_gap(self.settings.import_gap);
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
        let sidebar_rect = sidebar_rect(screen_rect);
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
        placeholder_world: egui::Pos2,
        width: u32,
        height: u32,
    ) {
        let scale =
            crate::jobs::initial_import_scale(width, height, self.settings.auto_image_scaling);
        self.canvas.add_import_placeholder(
            queue_id,
            placeholder_world,
            egui::vec2(width as f32 * scale, height as f32 * scale),
            label,
        );
    }

    fn import_layout_snapshot(&self) -> Result<Vec<crate::data::BoardLayoutItem>, String> {
        let mut layout = if let Some(library) = &self.library {
            library.load_board_layout_items()?
        } else {
            Vec::new()
        };
        layout.extend(self.canvas.import_placeholder_layout_items());
        Ok(layout)
    }

    fn reserve_import_world(
        &self,
        layout: &mut Vec<crate::data::BoardLayoutItem>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
        _viewport_world: egui::Rect,
    ) -> egui::Pos2 {
        let scale =
            crate::jobs::initial_import_scale(width, height, self.settings.auto_image_scaling);
        let size = egui::vec2(width as f32 * scale, height as f32 * scale);
        crate::import_placement::reserve_import_world(
            layout,
            desired_world,
            size,
            self.settings.import_gap,
            crate::import_placement::import_snap_distance_world(&self.canvas),
        )
    }

    fn normalize_import_url(raw: &str) -> Option<String> {
        let parsed = url::Url::parse(raw.trim()).ok()?;
        ((parsed.scheme() == "http") || (parsed.scheme() == "https")).then(|| parsed.to_string())
    }

    fn import_source_label(source: &ExternalImportSource) -> String {
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

    fn hovered_import_sources(&mut self, ui: &egui::Ui) -> Vec<ExternalImportSource> {
        let hovered_files = ui.input(|input| input.raw.hovered_files.clone());
        let mut sources = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_urls = std::collections::HashSet::new();

        for file in hovered_files {
            if let Some(path) = file.path {
                let path_text = path.to_string_lossy();
                if let Some(url) = Self::normalize_import_url(&path_text) {
                    if seen_urls.insert(url.clone()) {
                        sources.push(ExternalImportSource::Url(url));
                    }
                } else if seen_paths.insert(path.clone()) {
                    sources.push(ExternalImportSource::Path(path));
                }
            }
        }

        for path in crate::native_drop::hovered_paths() {
            if seen_paths.insert(path.clone()) {
                sources.push(ExternalImportSource::Path(path));
            }
        }

        for url in crate::native_drop::hovered_url_strings() {
            if seen_urls.insert(url.clone()) {
                sources.push(ExternalImportSource::Url(url));
            }
        }

        sources
    }

    fn dropped_import_sources(&mut self, ui: &egui::Ui) -> Vec<ExternalImportSource> {
        let dropped_files = ui.input(|input| input.raw.dropped_files.clone());
        let mut sources = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        let mut seen_urls = std::collections::HashSet::new();

        for file in dropped_files {
            if let Some(path) = file.path {
                let path_text = path.to_string_lossy();
                if let Some(url) = Self::normalize_import_url(&path_text) {
                    if seen_urls.insert(url.clone()) {
                        sources.push(ExternalImportSource::Url(url));
                    }
                } else if seen_paths.insert(path.clone()) {
                    sources.push(ExternalImportSource::Path(path));
                }
            } else if let Some(url) = Self::normalize_import_url(&file.name) {
                if seen_urls.insert(url.clone()) {
                    sources.push(ExternalImportSource::Url(url));
                }
            }
        }

        for path in crate::native_drop::take_dropped_paths() {
            if seen_paths.insert(path.clone()) {
                sources.push(ExternalImportSource::Path(path));
            }
        }

        for url in crate::native_drop::take_dropped_url_strings() {
            if seen_urls.insert(url.clone()) {
                sources.push(ExternalImportSource::Url(url));
            }
        }

        sources
    }

    fn handle_hovered_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        let hovered_sources = self.hovered_import_sources(ui);
        if hovered_sources.is_empty() {
            self.canvas.clear_hover_preview_placeholders();
            self.hover_preview_dimensions.clear();
            return;
        }

        let hovered_paths = hovered_sources
            .iter()
            .filter_map(|source| match source {
                ExternalImportSource::Path(path) => Some(path.clone()),
                ExternalImportSource::Url(_) => None,
            })
            .collect::<Vec<_>>();
        self.hover_preview_dimensions
            .retain(|path, _| hovered_paths.contains(path));

        let desired_world = self.preferred_import_world(ui.ctx(), screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(_) => return,
        };

        const FALLBACK_WIDTH: u32 = 360;
        const FALLBACK_HEIGHT: u32 = 240;

        let mut previews = Vec::with_capacity(hovered_sources.len());
        for source in hovered_sources {
            let (width, height) = match &source {
                ExternalImportSource::Path(path) => {
                    let dimensions = self
                        .hover_preview_dimensions
                        .entry(path.clone())
                        .or_insert_with(|| image::image_dimensions(path).ok());
                    dimensions.unwrap_or((FALLBACK_WIDTH, FALLBACK_HEIGHT))
                }
                ExternalImportSource::Url(_) => (FALLBACK_WIDTH, FALLBACK_HEIGHT),
            };
            let reserved_world = self.reserve_import_world(
                &mut layout,
                width,
                height,
                desired_world,
                viewport_world,
            );
            let scale =
                crate::jobs::initial_import_scale(width, height, self.settings.auto_image_scaling);
            let size_world = egui::vec2(width as f32 * scale, height as f32 * scale);
            previews.push((reserved_world, size_world));
        }

        self.canvas.set_hover_preview_placeholders(previews);
    }

    fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        if self.import_worker.is_none() {
            return;
        }

        let dropped_sources = self.dropped_import_sources(ui);
        if dropped_sources.is_empty() {
            return;
        }

        let desired_world = self.preferred_import_world(ui.ctx(), screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        const FALLBACK_WIDTH: u32 = 360;
        const FALLBACK_HEIGHT: u32 = 240;

        for source in dropped_sources {
            let queue_id = self.next_import_queue_id();
            let label = Self::import_source_label(&source);
            let dimensions = match &source {
                ExternalImportSource::Path(path) => image::image_dimensions(path).ok(),
                ExternalImportSource::Url(_) => Some((FALLBACK_WIDTH, FALLBACK_HEIGHT)),
            };
            let reserved_world = dimensions.map(|(width, height)| {
                let reserved_world = self.reserve_import_world(
                    &mut layout,
                    width,
                    height,
                    desired_world,
                    viewport_world,
                );
                self.queue_import_placeholder(
                    queue_id,
                    label.clone(),
                    reserved_world,
                    width,
                    height,
                );
                reserved_world
            });

            let request = match &source {
                ExternalImportSource::Path(source_path) => crate::jobs::ImportRequest::ImportPath {
                    queue_id,
                    source_path: source_path.clone(),
                    desired_world,
                    reserved_world,
                    viewport_world,
                    max_texture_side: self.max_texture_side,
                    canvas_image_settings: crate::jobs::CanvasImageSettings {
                        mode: self.settings.canvas_image_mode,
                        max_side: self.settings.canvas_image_max_side,
                    },
                    auto_scale: self.settings.auto_image_scaling,
                    import_gap: self.settings.import_gap,
                    trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                        queued_at: Instant::now(),
                        timings: crate::jobs::ImportTimings::default(),
                    }),
                },
                ExternalImportSource::Url(url) => crate::jobs::ImportRequest::ImportUrl {
                    queue_id,
                    url: url.clone(),
                    desired_world,
                    reserved_world,
                    viewport_world,
                    max_texture_side: self.max_texture_side,
                    canvas_image_settings: crate::jobs::CanvasImageSettings {
                        mode: self.settings.canvas_image_mode,
                        max_side: self.settings.canvas_image_max_side,
                    },
                    auto_scale: self.settings.auto_image_scaling,
                    import_gap: self.settings.import_gap,
                    trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                        queued_at: Instant::now(),
                        timings: crate::jobs::ImportTimings::default(),
                    }),
                },
            };

            if let Err(error) = self
                .import_worker
                .as_ref()
                .expect("checked import worker")
                .enqueue(request)
            {
                self.canvas.remove_import_placeholder(queue_id);
                self.status = error;
                continue;
            }

            self.status = match &source {
                ExternalImportSource::Path(path) => {
                    format!("Queued import: {}", path.display())
                }
                ExternalImportSource::Url(url) => format!("Queued URL import: {url}"),
            };
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

        if self.handle_clipboard_url_import(ctx, screen_rect, &paste_text) {
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
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let reserved_world =
            self.reserve_import_world(&mut layout, width, height, desired_world, viewport_world);
        self.queue_import_placeholder(
            queue_id,
            "Clipboard".to_owned(),
            reserved_world,
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
                reserved_world: Some(reserved_world),
                viewport_world,
                max_texture_side: self.max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    mode: self.settings.canvas_image_mode,
                    max_side: self.settings.canvas_image_max_side,
                },
                auto_scale: self.settings.auto_image_scaling,
                import_gap: self.settings.import_gap,
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

    fn handle_clipboard_url_import(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        paste_text: &str,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }
        let Some(url) = Self::normalize_import_url(paste_text) else {
            return false;
        };

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let queue_id = self.next_import_queue_id();
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return true;
            }
        };
        let reserved_world =
            self.reserve_import_world(&mut layout, 360, 240, desired_world, viewport_world);
        self.queue_import_placeholder(
            queue_id,
            Self::import_source_label(&ExternalImportSource::Url(url.clone())),
            reserved_world,
            360,
            240,
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(crate::jobs::ImportRequest::ImportUrl {
                queue_id,
                url: url.clone(),
                desired_world,
                reserved_world: Some(reserved_world),
                viewport_world,
                max_texture_side: self.max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    mode: self.settings.canvas_image_mode,
                    max_side: self.settings.canvas_image_max_side,
                },
                auto_scale: self.settings.auto_image_scaling,
                import_gap: self.settings.import_gap,
                trace: self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
                    queued_at: Instant::now(),
                    timings: crate::jobs::ImportTimings::default(),
                }),
            })
        {
            self.canvas.remove_import_placeholder(queue_id);
            self.status = error;
            return true;
        }

        self.status = format!("Queued URL import: {url}");
        true
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
                    self.residency.note_loaded_item(loaded.record.item.id);
                    if self
                        .canvas
                        .record_for_item_id(loaded.record.item.id)
                        .is_some_and(|current| current.item.asset_id != loaded.record.item.asset_id)
                    {
                        continue;
                    }
                    let mut trace = loaded.trace;
                    #[cfg(feature = "semantic-search")]
                    let queued_asset = self.settings.semantic_search_enabled.then(|| {
                        (
                            loaded.record.asset.id,
                            loaded.record.asset.original_path.clone(),
                        )
                    });
                    let ui_integration_started_at = Instant::now();
                    self.canvas.add_loaded_item(ctx, loaded);
                    #[cfg(feature = "semantic-search")]
                    if let Some((asset_id, original_path)) = queued_asset {
                        self.search.queue_asset_index(asset_id, original_path);
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
                        if let Some(error) = self.residency.queue_backfill(
                            self.import_worker.as_ref(),
                            &loaded_record,
                            &self.settings,
                            crate::data::CanvasImageCacheTier::Overview,
                        ) {
                            self.status = error;
                        }
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
                        self.settings.canvas_image_mode,
                        self.settings.canvas_image_max_side,
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
                            self.residency.note_loaded_item(updated_item_id);
                            self.push_crop_undo_entry(CropUndoEntry {
                                previous_record: previous_snapshot.0,
                                previous_rgba: previous_snapshot.1,
                                previous_preview_width: previous_snapshot.2,
                                previous_preview_height: previous_snapshot.3,
                                removed_asset_id: asset.id,
                            });
                            self.canvas.clear_crop_pending(updated_item_id);
                            #[cfg(feature = "semantic-search")]
                            if self.settings.semantic_search_enabled {
                                self.search
                                    .queue_asset_index(asset.id, asset.original_path.clone());
                            }
                            if let Some(error) = self.residency.queue_backfill(
                                self.import_worker.as_ref(),
                                &updated_record,
                                &self.settings,
                                crate::data::CanvasImageCacheTier::Overview,
                            ) {
                                self.status = error;
                            }
                            self.status = "Cropped image".to_owned();
                        }
                        Err(error) => {
                            self.canvas.clear_crop_pending(updated_item_id);
                            self.status = error;
                        }
                    }
                }
                crate::jobs::ImportResult::CacheTierBackfilled { asset_id, tier } => {
                    let _ = self.residency.note_backfill_completed(asset_id, tier, None);
                }
                crate::jobs::ImportResult::CacheTierBackfillFailed {
                    asset_id,
                    tier,
                    error,
                } => {
                    if let Some(error) =
                        self.residency
                            .note_backfill_completed(asset_id, tier, Some(error))
                    {
                        self.status = error;
                    }
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
                    self.residency.note_failed_item(&mut self.canvas, item_id);
                    if let Some(item_id) = item_id {
                        self.canvas.clear_crop_pending(item_id);
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
                animated_preview: None,
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
        if self.search.is_open() {
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
                            self.settings.import_gap,
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
            self.settings.import_gap,
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
                    mode: self.settings.canvas_image_mode,
                    max_side: self.settings.canvas_image_max_side,
                },
            }) {
                self.canvas.clear_crop_pending(item_id);
                self.status = error;
                continue;
            }

            self.status = "Cropping image".to_owned();
        }
    }

    fn handle_pending_annotation_requests(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_annotation() {
            let background_enabled = request.kind == crate::data::CanvasAnnotationKind::Textbox
                && self.settings.default_textbox_background;
            match library.create_canvas_annotation(
                request.kind,
                request.start_world,
                request.end_world,
                &request.text,
                request.text_size,
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                background_enabled,
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

        let changes = self.canvas.take_document_changes();
        if !changes.archived_annotation_ids.is_empty()
            && let Err(error) = library.archive_canvas_annotations(&changes.archived_annotation_ids)
        {
            self.status = error;
        }

        for item in changes.updated_items {
            if let Err(error) = library.update_board_item(&item) {
                self.status = error;
            }
        }

        for annotation in changes.updated_annotations {
            if let Err(error) = library.update_canvas_annotation(&annotation) {
                self.status = error;
            }
        }
    }

    fn save_user_session(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        let session = crate::data::UserSession {
            viewport: self.canvas.viewport_state(),
            settings: self.settings.clone(),
        };
        if let Err(error) = library.save_user_session(&session) {
            self.status = error;
        }
    }

    #[cfg(debug_assertions)]
    fn seed_background_verification(&mut self, mode: VerificationMode) {
        let (background, zoom) = match mode {
            VerificationMode::BackgroundDots => (crate::data::CanvasBackground::Dots, 1.0),
            VerificationMode::BackgroundTransition => {
                let transition_zoom = 0.9;
                (crate::data::CanvasBackground::Grid, transition_zoom)
            }
            _ => (crate::data::CanvasBackground::Grid, 1.0),
        };
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 0.0,
                camera_pan_y: 0.0,
                camera_zoom: zoom,
                canvas_background: background,
            });
    }

    #[cfg(debug_assertions)]
    fn seed_import_verification_existing_anchor(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<i64, String> {
        self.seed_import_verification_image(
            ctx,
            "verify-existing",
            egui::pos2(0.0, 0.0),
            180,
            140,
            [188, 146, 255, 255],
        )
    }

    #[cfg(debug_assertions)]
    fn seed_import_verification_image(
        &mut self,
        ctx: &egui::Context,
        source_name: &str,
        center_world: egui::Pos2,
        width: u32,
        height: u32,
        color: [u8; 4],
    ) -> Result<i64, String> {
        let Some(library) = &self.library else {
            return Err("Library unavailable".to_owned());
        };
        let rgba = Self::solid_rgba(width, height, color);
        let stored_path = library.store_rgba_image(source_name, &rgba, width, height)?;
        let file_size = std::fs::metadata(&stored_path)
            .map_err(|error| format!("failed to read debug image metadata: {error}"))?
            .len();
        let record = library.persist_imported_image(
            &stored_path,
            image::ImageFormat::Png.to_mime_type(),
            width,
            height,
            file_size,
            center_world,
            1.0,
        )?;
        let item_id = record.item.id;
        self.canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                queue_id: None,
                record,
                preview_rgba: rgba,
                preview_width: width,
                preview_height: height,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        Ok(item_id)
    }

    #[cfg(debug_assertions)]
    fn seed_rotation_verification(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        match self.seed_import_verification_existing_anchor(ctx) {
            Ok(item_id) => {
                self.canvas.select_items(&[item_id]);
                let _ = self.canvas.set_image_rotation(item_id, 30_f32.to_radians());
                self.status = "Added rotated debug image".to_owned();
            }
            Err(error) => self.status = error,
        }
    }

    #[cfg(debug_assertions)]
    fn seed_gif_verification(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        let width = 160_u32;
        let height = 120_u32;
        let first = Self::solid_rgba(width, height, [231, 94, 66, 255]);
        let second = Self::solid_rgba(width, height, [70, 142, 255, 255]);
        let asset = crate::data::ImageAsset {
            id: -40,
            original_path: std::path::PathBuf::from("/tmp/debug.gif"),
            filename: "debug.gif".to_owned(),
            mime_type: image::ImageFormat::Gif.to_mime_type().to_owned(),
            width,
            height,
            file_size: (first.len() + second.len()) as u64,
            imported_at: 0,
        };
        let record = crate::data::BoardItemWithAsset {
            item: crate::data::BoardItem {
                id: -40,
                asset_id: -40,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset,
        };
        self.canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                queue_id: None,
                record,
                preview_rgba: first.clone(),
                preview_width: width,
                preview_height: height,
                animated_preview: Some(crate::jobs::AnimatedPreview {
                    frames: vec![
                        crate::jobs::AnimatedPreviewFrame {
                            rgba: first,
                            delay_seconds: 0.12,
                        },
                        crate::jobs::AnimatedPreviewFrame {
                            rgba: second,
                            delay_seconds: 0.12,
                        },
                    ],
                    width,
                    height,
                }),
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        self.canvas.select_items(&[-40]);
        self.canvas.set_animation_time_override_seconds(Some(0.13));
        self.status = "Added animated GIF debug image".to_owned();
    }

    #[cfg(debug_assertions)]
    fn seed_move_snap_verification(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        self.canvas.clear_scene_for_verification();
        self.settings.import_gap = 24.0;
        self.canvas.set_layout_gap(self.settings.import_gap);

        let left_result = self.seed_import_verification_image(
            ctx,
            "verify-snap-left",
            egui::pos2(-170.0, 150.0),
            120,
            180,
            [126, 150, 255, 255],
        );
        let top_result = self.seed_import_verification_image(
            ctx,
            "verify-snap-top",
            egui::pos2(10.0, -120.0),
            220,
            120,
            [188, 146, 255, 255],
        );
        let moving_result = self.seed_import_verification_image(
            ctx,
            "verify-snap-moving",
            egui::pos2(70.0, 36.0),
            180,
            140,
            [231, 94, 66, 255],
        );

        match (left_result, top_result, moving_result) {
            (Ok(_), Ok(_), Ok(item_id)) => {
                self.canvas.select_items(&[item_id]);
                let screen_rect =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, STARTUP_VIEWPORT_SIZE);
                if self
                    .canvas
                    .seed_move_snap_preview_for_selected_items(screen_rect, egui::vec2(0.0, -88.0))
                {
                    self.status = "Added move snap debug preview".to_owned();
                } else {
                    self.status = "Failed to compute move snap debug preview".to_owned();
                }
            }
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => self.status = error,
        }
    }

    #[cfg(debug_assertions)]
    fn seed_import_verification_placeholders(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        if let Err(error) = self.seed_import_verification_existing_anchor(ctx) {
            self.status = error;
            return;
        }
        let desired_world = egui::Pos2::ZERO;
        let viewport_world = self.startup_viewport_world_rect();
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        for (index, (label, width, height)) in [
            ("Alpha", 180_u32, 140_u32),
            ("Beta", 180, 140),
            ("Gamma", 180, 140),
        ]
        .into_iter()
        .enumerate()
        {
            let queue_id = self.next_import_queue_id();
            let reserved_world = self.reserve_import_world(
                &mut layout,
                width,
                height,
                desired_world,
                viewport_world,
            );
            self.queue_import_placeholder(
                queue_id,
                label.to_owned(),
                reserved_world,
                width,
                height,
            );
            self.status = format!("Queued debug import placeholder {}", index + 1);
        }
    }

    #[cfg(debug_assertions)]
    fn seed_import_verification_loaded(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        if self.import_worker.is_none() {
            self.status = "Import worker unavailable".to_owned();
            return;
        }
        if let Err(error) = self.seed_import_verification_existing_anchor(ctx) {
            self.status = error;
            return;
        }
        let desired_world = egui::Pos2::ZERO;
        let viewport_world = self.startup_viewport_world_rect();
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        for (label, width, height, color) in [
            ("Alpha", 180_u32, 140_u32, [231_u8, 94, 66, 255]),
            ("Beta", 180, 140, [70, 142, 255, 255]),
            ("Gamma", 180, 140, [73, 179, 122, 255]),
        ] {
            let queue_id = self.next_import_queue_id();
            let reserved_world = self.reserve_import_world(
                &mut layout,
                width,
                height,
                desired_world,
                viewport_world,
            );
            self.queue_import_placeholder(
                queue_id,
                label.to_owned(),
                reserved_world,
                width,
                height,
            );
            let rgba = Self::solid_rgba(width, height, color);
            let enqueue_result = self.import_worker.as_ref().and_then(|import_worker| {
                import_worker
                    .enqueue(crate::jobs::ImportRequest::ImportClipboard {
                        queue_id,
                        rgba,
                        width,
                        height,
                        desired_world,
                        reserved_world: Some(reserved_world),
                        viewport_world,
                        max_texture_side: self.max_texture_side,
                        canvas_image_settings: crate::jobs::CanvasImageSettings {
                            mode: self.settings.canvas_image_mode,
                            max_side: self.settings.canvas_image_max_side,
                        },
                        auto_scale: self.settings.auto_image_scaling,
                        import_gap: self.settings.import_gap,
                        trace: None,
                    })
                    .err()
            });
            if let Some(error) = enqueue_result {
                self.canvas.remove_import_placeholder(queue_id);
                self.status = error;
                return;
            }
        }
        self.status = "Queued debug imports".to_owned();
    }

    #[cfg(debug_assertions)]
    fn verification_imports_settled(&self) -> bool {
        !matches!(self.verification_mode, Some(VerificationMode::ImportLoaded))
            || self.canvas.import_placeholder_count() == 0
    }

    #[cfg(not(debug_assertions))]
    fn verification_imports_settled(&self) -> bool {
        true
    }

    fn solid_rgba(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut rgba = vec![0; width as usize * height as usize * 4];
        for chunk in rgba.chunks_exact_mut(4) {
            chunk.copy_from_slice(&color);
        }
        rgba
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn drive_verification_search(&mut self) {
        let Some(query) = self.verification_search_query.clone() else {
            return;
        };
        self.search.drive_verification_search(
            &query,
            &mut self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
        );
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn verification_search_settled(&self) -> bool {
        self.verification_search_query
            .as_deref()
            .is_none_or(|query| self.search.verification_search_settled(query))
    }

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn drive_verification_search(&mut self) {}

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn verification_search_settled(&self) -> bool {
        true
    }

    #[cfg(feature = "semantic-search")]
    fn sync_semantic_search_service(&mut self) {
        self.search.sync_service(
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.status,
            Some(&mut self.canvas),
        );
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_results(&mut self) {
        self.search.handle_results();
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_shortcut(&mut self, ctx: &egui::Context) {
        self.search.handle_shortcut(
            ctx,
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
        );
    }

    #[cfg(feature = "semantic-search")]
    fn show_search_overlay(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        if let Some(drop_request) = self.search.show_overlay(ctx, screen_rect, &mut self.canvas) {
            self.handle_search_drop(drop_request, screen_rect);
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_drop(
        &mut self,
        drop_request: crate::search::SearchDropRequest,
        screen_rect: egui::Rect,
    ) {
        if drop_request.item_ids.is_empty() {
            return;
        }

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let drop_world = self
            .canvas
            .screen_to_world(drop_request.screen_pos, screen_rect);
        let drop_world = self
            .canvas
            .take_search_drag_drop_anchor_world()
            .unwrap_or(drop_world);
        let source_center = self
            .source_center_for_item_ids(&drop_request.item_ids)
            .unwrap_or(drop_world);
        let duplicated = match library.duplicate_board_items(
            &drop_request.item_ids,
            drop_world,
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
        if duplicated_ids.len() > 1 {
            self.canvas
                .arrange_selected_compact(self.settings.import_gap);
        }
        self.persist_canvas_changes();
        let noun = if duplicated_ids.len() == 1 {
            "image"
        } else {
            "images"
        };
        self.status = format!("Added {} search {noun}", duplicated_ids.len());
        self.canvas.clear_search_drag_preview();
        self.search.close_overlay(Some(&mut self.canvas));
    }

    #[cfg(feature = "semantic-search")]
    fn source_center_for_item_ids(&self, item_ids: &[i64]) -> Option<egui::Pos2> {
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut found = false;

        for item_id in item_ids {
            let Some(record) = self.canvas.record_for_item_id(*item_id) else {
                continue;
            };
            found = true;
            let half_size = egui::vec2(
                record.asset.width as f32 * record.item.scale.max(0.01),
                record.asset.height as f32 * record.item.scale.max(0.01),
            ) * 0.5;
            min.x = min.x.min(record.item.x - half_size.x);
            min.y = min.y.min(record.item.y - half_size.y);
            max.x = max.x.max(record.item.x + half_size.x);
            max.y = max.y.max(record.item.y + half_size.y);
        }

        found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
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
            crate::jobs::ImportOrigin::Url => {
                if let Some(trace) = trace {
                    eprintln!("[perf] url_import{}", format_trace(trace));
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

        #[cfg(debug_assertions)]
        if matches!(
            self.verification_mode,
            Some(VerificationMode::ImportPlaceholders)
        ) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }

        if !self.verification_search_settled() || !self.verification_imports_settled() {
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
        self.handle_hovered_files(ui, screen_rect);
        self.handle_dropped_files(ui, screen_rect);
        #[cfg(feature = "semantic-search")]
        self.handle_search_shortcut(ui.ctx());
        self.handle_selection_shortcuts(ui.ctx(), screen_rect);
        let sidebar_open = self.sidebar_open;
        if sidebar_open {
            block_sidebar_input(ui.ctx(), sidebar_rect(screen_rect));
        }
        self.canvas
            .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
        self.canvas.set_layout_gap(self.settings.import_gap);
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        #[cfg(feature = "semantic-search")]
        let suppress_canvas_input = self.search.blocks_canvas_input(ui.ctx());
        #[cfg(not(feature = "semantic-search"))]
        let suppress_canvas_input = false;
        self.frame_perf.canvas = crate::canvas::show(
            ui,
            screen_rect,
            visible_canvas_rect,
            &mut self.canvas,
            self.wgpu_render_state.as_ref(),
            self.settings.canvas_image_mode == crate::data::CanvasImageMode::Downscale,
            suppress_canvas_input,
            false,
        );
        if let Some(error) = self.residency.dispatch_pending_tier_requests(
            self.import_worker.as_ref(),
            &mut self.canvas,
            &self.settings,
            self.max_texture_side,
        ) {
            self.status = error;
        }
        self.handle_pending_crop_requests();
        self.handle_pending_annotation_requests();
        self.persist_canvas_changes();
        let sidebar_result = show_sidebar(
            ui.ctx(),
            screen_rect,
            sidebar_open,
            &mut self.sidebar_tab,
            &mut self.settings,
            &mut self.canvas,
        );
        if sidebar_open {
            close_sidebar_on_outside_click(
                ui.ctx(),
                sidebar_result
                    .interactive_rect
                    .unwrap_or_else(|| sidebar_rect(screen_rect)),
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

#[cfg(test)]
mod tests {
    use super::{ArrangementAction, ReferenceApp, apply_arrangement_action};
    use crate::data::{BoardItem, BoardItemWithAsset, ImageAsset};
    use crate::jobs::LoadedCanvasItem;
    #[cfg(feature = "semantic-search")]
    use crate::palette::{
        PALETTE_WIDTH, search_result_grid_content_height, search_result_grid_layout,
        search_result_tile_height,
    };
    use crate::sidebar::should_close_sidebar;
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
        app.settings.auto_image_scaling = false;
        app.settings.import_gap = 37.0;
        app.settings.canvas_image_mode = crate::data::CanvasImageMode::Original;
        app.settings.canvas_image_max_side = 4096;
        app.canvas
            .set_background(crate::data::CanvasBackground::Grid);

        app.save_user_session();

        let session = library.load_user_session().unwrap();
        assert!(!session.settings.auto_image_scaling);
        assert_eq!(session.settings.import_gap, 37.0);
        assert_eq!(
            session.settings.canvas_image_mode,
            crate::data::CanvasImageMode::Original
        );
        assert_eq!(session.settings.canvas_image_max_side, 4096);
        assert_eq!(
            session.viewport.canvas_background,
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
                    animated_preview: None,
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
        app.settings.semantic_search_enabled = true;

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

        assert!(app.search.overlay.open);
        assert!(app.search.overlay.focus_requested);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn escape_closes_search_overlay_and_clears_matches() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.search.overlay.open = true;
        app.search.overlay.query = "cat".to_owned();
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

        assert!(!app.search.overlay.open);
        assert!(app.search.overlay.query.is_empty());
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_overlay_focuses_text_input_when_requested() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.search.overlay.open = true;
        app.search.overlay.focus_requested = true;

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        });

        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("semantic_search_input"))));
        assert!(!app.search.overlay.focus_requested);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_result_grid_layout_balances_five_results_into_five_columns() {
        let shell_content_width = PALETTE_WIDTH - 24.0;
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

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_drop_source_center_uses_image_extents() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);

        for (id, x, width) in [(1_i64, 0.0_f32, 100_u32), (2, 200.0, 300)] {
            let asset = ImageAsset {
                id,
                original_path: PathBuf::from(format!("fixture-{id}.png")),
                filename: format!("fixture-{id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height: 100,
                file_size: 0,
                imported_at: 0,
            };
            let record = BoardItemWithAsset {
                item: BoardItem {
                    id,
                    asset_id: id,
                    x,
                    y: 0.0,
                    scale: 1.0,
                    rotation: 0.0,
                    z_index: id as i32,
                    archived: false,
                },
                asset,
            };
            app.canvas.add_loaded_item(
                &ctx,
                LoadedCanvasItem {
                    queue_id: None,
                    record,
                    preview_rgba: vec![255; width as usize * 100 * 4],
                    preview_width: width,
                    preview_height: 100,
                    animated_preview: None,
                    preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                    loaded_tier: crate::data::CanvasImageCacheTier::Working,
                    existing_load_diagnostics: None,
                    origin: crate::jobs::ImportOrigin::Existing,
                    trace: None,
                },
            );
        }

        let center = app.source_center_for_item_ids(&[1, 2]).unwrap();

        assert_eq!(center, egui::pos2(150.0, 0.0));
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

    #[test]
    fn reserve_import_world_accounts_for_pending_placeholders() {
        let mut app = test_app(None);
        app.queue_import_placeholder(1, "first".to_owned(), egui::pos2(0.0, 0.0), 100, 100);

        let mut layout = app.import_layout_snapshot().unwrap();
        let reserved = app.reserve_import_world(
            &mut layout,
            100,
            100,
            egui::pos2(0.0, 0.0),
            test_viewport_world(),
        );

        assert_eq!(reserved, egui::pos2(100.0 + app.settings.import_gap, 0.0));
    }

    #[test]
    fn normalize_import_url_accepts_http_and_https_only() {
        assert_eq!(
            ReferenceApp::normalize_import_url(" https://example.com/cat.png?size=large "),
            Some("https://example.com/cat.png?size=large".to_owned())
        );
        assert_eq!(
            ReferenceApp::normalize_import_url("http://example.com/dog.gif"),
            Some("http://example.com/dog.gif".to_owned())
        );
        assert_eq!(
            ReferenceApp::normalize_import_url("file:///tmp/cat.png"),
            None
        );
        assert_eq!(ReferenceApp::normalize_import_url("not a url"), None);
    }

    #[test]
    fn import_source_label_prefers_url_path_basename_then_host() {
        assert_eq!(
            ReferenceApp::import_source_label(&super::ExternalImportSource::Url(
                "https://images.example.com/path/cat.gif?size=large".to_owned()
            )),
            "cat.gif"
        );
        assert_eq!(
            ReferenceApp::import_source_label(&super::ExternalImportSource::Url(
                "https://images.example.com".to_owned()
            )),
            "images.example.com"
        );
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
            settings: crate::data::WorkspaceSettings {
                semantic_search_enabled: false,
                ..Default::default()
            },
            #[cfg(feature = "semantic-search")]
            search: super::SearchController::default(),
            frame_perf: super::FramePerfState::default(),
            residency: super::ImageResidencyCoordinator::default(),
            sidebar_open: false,
            sidebar_tab: super::SidebarTab::default(),
            status: String::new(),
            perf_log: None,
            next_import_queue_id: 1,
            hover_preview_dimensions: std::collections::HashMap::new(),
            #[cfg(debug_assertions)]
            verification_mode: None,
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

    fn test_viewport_world() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0))
    }
}
