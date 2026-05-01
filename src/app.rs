use eframe::egui;
use std::time::Instant;

use crate::active_board_commands::{
    ActiveBoardCommandContext, ClipboardBoardSelection, PasteCommandOutcome,
};
use crate::crop_flow::CropFlow;
use crate::document_persistence::DocumentPersistence;
use crate::import_intake::{
    ExternalImportSource, ImportIntake, ImportPlacementSettings, ImportRequestConfig,
};
#[cfg(feature = "semantic-search")]
use crate::palette::ImageSearchPivot;
use crate::residency::{
    FailedImportOutcome, ImageResidencyCoordinator, ImportResultResidencyOutcome,
    LoadedImportOutcome,
};
#[cfg(feature = "semantic-search")]
use crate::search::SearchController;
use crate::sidebar::{
    SidebarBoardAction, SidebarTab, block_sidebar_input, close_sidebar_on_outside_click,
    show_sidebar, show_sidebar_hint, sidebar_rect,
};

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
    MoveSelectionToFront,
    MoveSelectionToBack,
    ToggleSidebar,
    FocusSelection,
    Arrange(ArrangementAction),
    ApplyDefaultAverageArea,
    AverageSelectionArea,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContextMenuAction {
    Copy,
    Paste,
    Duplicate,
    DeleteFromBoard,
    MoveToFront,
    MoveToBack,
    SearchUsingImage,
    RemoveFromLibrary,
}

impl ContextMenuAction {
    fn is_destructive(self) -> bool {
        matches!(self, Self::DeleteFromBoard | Self::RemoveFromLibrary)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ContextMenuItem {
    action: ContextMenuAction,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
}

#[derive(Clone, Copy)]
struct ShortcutBinding {
    key: egui::Key,
    command: bool,
    shift: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingBoardDelete {
    board_id: i64,
    board_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingLibraryRemove {
    asset_ids: Vec<i64>,
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
        action: "Move selection to front",
        windows: "Ctrl+.",
        mac: "⌘.",
        binding: Some(ShortcutBinding {
            key: egui::Key::Period,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToFront),
    },
    SupportedShortcut {
        action: "Move selection to back",
        windows: "Ctrl+,",
        mac: "⌘,",
        binding: Some(ShortcutBinding {
            key: egui::Key::Comma,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::MoveSelectionToBack),
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

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub screenshot_path: Option<std::path::PathBuf>,
    perf_log: bool,
    perf_report: bool,
    perf_decode_path: Option<std::path::PathBuf>,
    perf_import_path: Option<std::path::PathBuf>,
    startup_trace: bool,
    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    search_latency_log: bool,
    #[cfg(debug_assertions)]
    verification_mode: Option<VerificationMode>,
    #[cfg(debug_assertions)]
    verification_search_query: Option<String>,
    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    verification_search_scope: crate::semantic_search::SearchScope,
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
    OpenSidebarBoardsHover,
    OpenPalette,
    OpenPaletteImagePivot,
    DrawArrow,
    EditTextbox,
    RotateImage,
    AnimatedGif,
    SvgImage,
    MoveSnapPreview,
    ContextMenu,
    ImportPlaceholders,
    ImportLoaded,
    BackgroundGrid,
    BackgroundDots,
    BackgroundTransition,
    ActiveBoardChip,
    SearchLatency,
    BoardDeleteConfirmation,
    LibraryRemoveConfirmation,
    DesignSystemPreview,
    CompactControlsComparison,
    RadiusComparison,
    DestructiveActionsComparison,
    SpacingComparison,
    BoardRowsComparison,
    BoardRowsComparisonBottom,
}

#[cfg(all(debug_assertions, feature = "semantic-search"))]
#[derive(Clone, Debug)]
struct SearchLatencyProbe {
    phase: SearchLatencyProbePhase,
    selected_item_id: Option<i64>,
    probe_started_at: Instant,
    command_started_at: Option<Instant>,
    tab_started_at: Option<Instant>,
    board_result_ready_ms: Option<f64>,
    library_result_ready_ms: Option<f64>,
    board_results_ms: Option<f64>,
}

#[cfg(all(debug_assertions, feature = "semantic-search"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchLatencyProbePhase {
    WaitExistingImage,
    OpenSearch,
    WaitBoardResults,
    WaitBoardPaint,
    SwitchToLibrary,
    WaitLibraryResults,
    WaitLibraryPaint,
    Done,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut screenshot_path = None;
        let mut perf_log = false;
        let mut perf_report = false;
        let mut perf_decode_path = None;
        let mut perf_import_path = None;
        let mut startup_trace = false;
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        let mut search_latency_log = std::env::var_os("REFERENCE_SEARCH_LATENCY_LOG").is_some();
        #[cfg(debug_assertions)]
        let mut verification_mode = None;
        #[cfg(debug_assertions)]
        let mut verification_search_query = None;
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        let mut verification_search_scope = crate::semantic_search::SearchScope::ThisBoard;
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
            } else if argument == "--perf-decode" {
                perf_decode_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-decode=") {
                perf_decode_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-import" {
                perf_import_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-import=") {
                perf_import_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--startup-trace" {
                startup_trace = true;
            } else if cfg!(all(debug_assertions, feature = "semantic-search"))
                && argument == "--search-latency-log"
            {
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                {
                    search_latency_log = true;
                }
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
            } else if cfg!(debug_assertions) && argument == "--verify-sidebar-boards-hover" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::OpenSidebarBoardsHover);
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
            } else if cfg!(debug_assertions) && argument == "--verify-svg" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::SvgImage);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-move-snap" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::MoveSnapPreview);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-context-menu" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::ContextMenu);
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
            } else if cfg!(debug_assertions) && argument == "--verify-active-board-chip" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::ActiveBoardChip);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-search-latency" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::SearchLatency);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-board-delete-confirmation" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BoardDeleteConfirmation);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-library-remove-confirmation" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::LibraryRemoveConfirmation);
                }
            } else if cfg!(debug_assertions) && argument == "--verify-design-system" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::DesignSystemPreview);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-compact-controls" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::CompactControlsComparison);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-radii" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::RadiusComparison);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-destructive-actions" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::DestructiveActionsComparison);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-spacing" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::SpacingComparison);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-board-rows" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BoardRowsComparison);
                }
            } else if cfg!(debug_assertions) && argument == "--compare-board-rows-bottom" {
                #[cfg(debug_assertions)]
                {
                    verification_mode = Some(VerificationMode::BoardRowsComparisonBottom);
                }
            } else if let Some(query) = argument.strip_prefix("--verify-search=") {
                #[cfg(debug_assertions)]
                {
                    verification_search_query = Some(query.to_owned());
                }
            } else if let Some(query) = argument.strip_prefix("--verify-search-library=") {
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                {
                    verification_search_query = Some(query.to_owned());
                    verification_search_scope = crate::semantic_search::SearchScope::Library;
                }
            } else if cfg!(debug_assertions) && argument == "--verify-search" {
                #[cfg(debug_assertions)]
                {
                    verification_search_query = arguments.next();
                }
            } else if cfg!(debug_assertions) && argument == "--verify-search-library" {
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                {
                    verification_search_query = arguments.next();
                    verification_search_scope = crate::semantic_search::SearchScope::Library;
                }
            }
        }

        Self {
            screenshot_path,
            perf_log,
            perf_report,
            perf_decode_path,
            perf_import_path,
            startup_trace,
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            search_latency_log,
            #[cfg(debug_assertions)]
            verification_mode,
            #[cfg(debug_assertions)]
            verification_search_query,
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            verification_search_scope,
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
                    | VerificationMode::OpenSidebarBoardsHover
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
            Some(VerificationMode::OpenSidebarBoards | VerificationMode::OpenSidebarBoardsHover)
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
    fn verify_svg_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::SvgImage))
    }

    #[cfg(debug_assertions)]
    fn verify_move_snap_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::MoveSnapPreview)
        )
    }

    #[cfg(debug_assertions)]
    fn verify_context_menu_on_start(&self) -> bool {
        matches!(self.verification_mode, Some(VerificationMode::ContextMenu))
    }

    #[cfg(debug_assertions)]
    fn verify_board_delete_confirmation_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::BoardDeleteConfirmation)
        )
    }

    #[cfg(debug_assertions)]
    fn verify_library_remove_confirmation_on_start(&self) -> bool {
        matches!(
            self.verification_mode,
            Some(VerificationMode::LibraryRemoveConfirmation)
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
                    | VerificationMode::SvgImage
                    | VerificationMode::MoveSnapPreview
                    | VerificationMode::ContextMenu
                    | VerificationMode::ImportPlaceholders
                    | VerificationMode::ImportLoaded
                    | VerificationMode::BackgroundGrid
                    | VerificationMode::BackgroundDots
                    | VerificationMode::BackgroundTransition
                    | VerificationMode::BoardDeleteConfirmation
                    | VerificationMode::LibraryRemoveConfirmation
                    | VerificationMode::DesignSystemPreview
                    | VerificationMode::CompactControlsComparison
                    | VerificationMode::RadiusComparison
                    | VerificationMode::DestructiveActionsComparison
                    | VerificationMode::SpacingComparison
                    | VerificationMode::BoardRowsComparison
                    | VerificationMode::BoardRowsComparisonBottom
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
        if matches!(
            self.verification_mode,
            Some(
                VerificationMode::BoardRowsComparison | VerificationMode::BoardRowsComparisonBottom
            )
        ) {
            egui::vec2(1720.0, 1480.0)
        } else if matches!(
            self.verification_mode,
            Some(
                VerificationMode::CompactControlsComparison
                    | VerificationMode::RadiusComparison
                    | VerificationMode::DestructiveActionsComparison
                    | VerificationMode::SpacingComparison
                    | VerificationMode::BoardRowsComparison
                    | VerificationMode::BoardRowsComparisonBottom
            )
        ) {
            egui::vec2(1180.0, 1040.0)
        } else if self.open_sidebar_narrow_on_start() {
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

    pub fn perf_decode_path(&self) -> Option<&std::path::Path> {
        self.perf_decode_path.as_deref()
    }

    pub fn perf_import_path(&self) -> Option<&std::path::Path> {
        self.perf_import_path.as_deref()
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
    active_board_id: i64,
    crop_flow: CropFlow,
    max_texture_side: usize,
    settings: crate::data::WorkspaceSettings,
    #[cfg(feature = "semantic-search")]
    search: SearchController,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    pending_board_delete: Option<PendingBoardDelete>,
    pending_library_remove: Option<PendingLibraryRemove>,
    context_menu_pos: Option<egui::Pos2>,
    status: String,
    perf_log: Option<PerfLogState>,
    frame_perf: FramePerfState,
    import_intake: ImportIntake,
    residency: ImageResidencyCoordinator,
    hover_preview_dimensions: std::collections::HashMap<std::path::PathBuf, Option<(u32, u32)>>,
    document_persistence: DocumentPersistence,
    #[cfg(debug_assertions)]
    verification_mode: Option<VerificationMode>,
    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    search_latency_probe: Option<SearchLatencyProbe>,
    #[cfg(debug_assertions)]
    verification_search_query: Option<String>,
    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    verification_search_scope: crate::semantic_search::SearchScope,
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
                    active_board_id: 1,
                    crop_flow: CropFlow::default(),
                    max_texture_side,
                    settings: crate::data::WorkspaceSettings::default(),
                    #[cfg(feature = "semantic-search")]
                    search: SearchController::default(),
                    sidebar_open: config.open_sidebar_on_start(),
                    sidebar_tab: if config.open_sidebar_boards_on_start() {
                        SidebarTab::Boards
                    } else {
                        SidebarTab::Options
                    },
                    pending_board_delete: None,
                    pending_library_remove: None,
                    context_menu_pos: None,
                    status,
                    perf_log: config.perf_log_enabled().then(|| PerfLogState {
                        app_started_at: Instant::now(),
                        startup_logged: false,
                    }),
                    frame_perf: FramePerfState::default(),
                    import_intake: ImportIntake::new(),
                    residency: ImageResidencyCoordinator::default(),
                    hover_preview_dimensions: std::collections::HashMap::new(),
                    document_persistence: DocumentPersistence::default(),
                    #[cfg(debug_assertions)]
                    verification_mode: config.verification_mode,
                    #[cfg(all(debug_assertions, feature = "semantic-search"))]
                    search_latency_probe: None,
                    #[cfg(debug_assertions)]
                    verification_search_query: None,
                    #[cfg(all(debug_assertions, feature = "semantic-search"))]
                    verification_search_scope: config.verification_search_scope,
                    #[cfg(debug_assertions)]
                    sidebar_hot_preview: config.open_sidebar_hot_on_start(),
                };
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                app.search
                    .set_latency_logging_enabled(config.search_latency_log);
                app.load_user_session();
                if config.open_sidebar_on_start() {
                    app.sidebar_open = true;
                }
                if config.open_sidebar_boards_on_start() {
                    app.sidebar_tab = SidebarTab::Boards;
                }
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
                if config.verify_svg_on_start() {
                    app.seed_svg_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_move_snap_on_start() {
                    app.seed_move_snap_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_context_menu_on_start() {
                    app.seed_context_menu_verification(&cc.egui_ctx);
                }
                #[cfg(debug_assertions)]
                if config.verify_board_delete_confirmation_on_start() {
                    app.pending_board_delete = Some(PendingBoardDelete {
                        board_id: app.active_board_id,
                        board_name: "Moodboard".to_owned(),
                    });
                }
                #[cfg(debug_assertions)]
                if config.verify_library_remove_confirmation_on_start() {
                    app.pending_library_remove = Some(PendingLibraryRemove {
                        asset_ids: vec![1, 2],
                    });
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
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                if matches!(
                    config.verification_mode,
                    Some(VerificationMode::SearchLatency)
                ) {
                    app.seed_search_latency_probe();
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
                        app.search.overlay.scope = config.verification_search_scope;
                        app.search.overlay.last_query_edit_at = Some(Instant::now());
                    }
                    app.verification_search_query =
                        config.verification_search_query().map(ToOwned::to_owned);
                }
                #[cfg(debug_assertions)]
                if app.sidebar_hot_preview {
                    app.settings.auto_image_scaling = false;
                    app.settings.import_gap = 32.0;
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
                active_board_id: 1,
                crop_flow: CropFlow::default(),
                max_texture_side,
                settings: crate::data::WorkspaceSettings::default(),
                #[cfg(feature = "semantic-search")]
                search: SearchController::default(),
                sidebar_open: config.open_sidebar_on_start(),
                sidebar_tab: if config.open_sidebar_boards_on_start() {
                    SidebarTab::Boards
                } else {
                    SidebarTab::Options
                },
                pending_board_delete: None,
                pending_library_remove: None,
                context_menu_pos: None,
                status: format!("Storage unavailable: {error}"),
                perf_log: config.perf_log_enabled().then(|| PerfLogState {
                    app_started_at: Instant::now(),
                    startup_logged: false,
                }),
                frame_perf: FramePerfState::default(),
                import_intake: ImportIntake::new(),
                residency: ImageResidencyCoordinator::default(),
                hover_preview_dimensions: std::collections::HashMap::new(),
                document_persistence: DocumentPersistence::default(),
                #[cfg(debug_assertions)]
                verification_mode: config.verification_mode,
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                search_latency_probe: None,
                #[cfg(debug_assertions)]
                verification_search_query: None,
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                verification_search_scope: config.verification_search_scope,
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

        match library.load_canvas_stack_order() {
            Ok(stack_order) => self.canvas.restore_canvas_stack_order(stack_order),
            Err(error) => self.status = error,
        }

        match library.load_board_items() {
            Ok(records) => {
                let viewport_world = self.startup_viewport_world_rect();
                if let Err(error) = self.residency.queue_startup_existing_loads(
                    import_worker,
                    records,
                    viewport_world,
                    self.active_board_id,
                    &self.settings,
                    self.max_texture_side,
                    self.perf_log.is_some(),
                ) {
                    self.status = error;
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

    fn board_summaries(&mut self) -> Vec<crate::data::BoardSummary> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        match library.list_board_summaries() {
            Ok(boards) => boards,
            Err(error) => {
                self.status = error;
                Vec::new()
            }
        }
    }

    fn create_and_switch_board(&mut self, keep_sidebar_open: bool) {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let boards = match library.list_boards() {
            Ok(boards) => boards,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let name = generated_board_name(&boards);
        let board = match library.create_board(&name) {
            Ok(board) => board,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        self.switch_to_board(board.id);
        if keep_sidebar_open {
            self.sidebar_open = true;
        }
        self.status = format!("Created {}", board.name);
    }

    fn switch_to_board(&mut self, board_id: i64) {
        if let Err(error) = crate::board_switch::switch_to_board(self, board_id) {
            self.status = error;
        }
    }

    fn handle_board_action(&mut self, action: Option<SidebarBoardAction>) {
        match action {
            Some(SidebarBoardAction::CreateBoard) => self.create_and_switch_board(true),
            Some(SidebarBoardAction::SwitchBoard(board_id)) => self.switch_to_board(board_id),
            Some(SidebarBoardAction::RenameBoard(board_id, name)) => {
                let Some(library) = &self.library else {
                    self.status = "Storage unavailable".to_owned();
                    return;
                };
                match library.rename_board(board_id, &name) {
                    Ok(()) => self.status = "Renamed board".to_owned(),
                    Err(error) => self.status = error,
                }
            }
            Some(SidebarBoardAction::RequestDeleteBoard(board_id)) => {
                let board_name = self
                    .board_summaries()
                    .into_iter()
                    .find(|summary| summary.board.id == board_id)
                    .map(|summary| summary.board.name)
                    .unwrap_or_else(|| "Board".to_owned());
                self.request_board_delete(board_id, board_name);
            }
            None => {}
        }
    }

    fn request_board_delete(&mut self, board_id: i64, board_name: String) {
        self.pending_board_delete = Some(PendingBoardDelete {
            board_id,
            board_name,
        });
    }

    fn cancel_pending_board_delete(&mut self) {
        self.pending_board_delete = None;
    }

    fn confirm_pending_board_delete(&mut self) {
        let Some(pending) = self.pending_board_delete.take() else {
            return;
        };

        self.save_active_board_before_transition();

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let deleted_active_board = pending.board_id == self.active_board_id;
        if let Err(error) = library.delete_board(pending.board_id) {
            self.status = error;
            return;
        }

        if deleted_active_board {
            self.load_active_board();
        }

        self.status = format!("Deleted {}", pending.board_name);
    }

    fn save_active_board_before_transition(&mut self) {
        self.persist_canvas_changes();
        self.save_user_session();
    }

    fn load_active_board(&mut self) {
        crate::active_board_load::load_active_board(self);
    }

    fn clear_active_board_transients(&mut self) {
        #[cfg(feature = "semantic-search")]
        self.search.close_overlay(Some(&mut self.canvas));
        #[cfg(not(feature = "semantic-search"))]
        {
            self.canvas.clear_search_matches();
            self.canvas.clear_search_drag_preview();
        }
        self.crop_flow.clear_undo_stack();
    }

    fn show_board_delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_board_delete.clone() else {
            return;
        };

        egui::Window::new("Delete board?")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(destructive_confirmation_frame(ctx))
            .show(ctx, |ui| {
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, "Delete board?");
                destructive_dialog_body(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("Delete \"{}\"?", pending.board_name))
                            .color(crate::theme::TEXT),
                    );
                    ui.add_space(crate::theme::SPACE_1);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Images stay in the shared Library. Placements, arrows, lines, and textboxes on this board will be removed.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.cancel_pending_board_delete();
                        }
                        if ui.add(destructive_action_button("Delete board")).clicked() {
                            self.confirm_pending_board_delete();
                        }
                    });
                });
            });
    }

    fn modal_open(&self) -> bool {
        self.pending_board_delete.is_some() || self.pending_library_remove.is_some()
    }

    fn should_block_sidebar_input(&self) -> bool {
        self.sidebar_open && !self.modal_open()
    }

    fn load_user_session(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_user_session() {
            Ok(session) => {
                self.canvas.apply_viewport_state(&session.viewport);
                self.active_board_id = session.active_board_id;
                self.settings = session.settings;
                self.settings.normalize_core_features();
                self.sidebar_open = session.sidebar_open;
                self.sidebar_tab = sidebar_tab_from_session(&session.sidebar_tab);
                self.canvas
                    .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
                self.canvas.set_layout_gap(self.settings.import_gap);
                self.document_persistence
                    .record_saved_user_session(self.current_user_session());
            }
            Err(error) => self.status = error,
        }
    }

    fn load_session_viewport(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_session_viewport() {
            Ok((active_board_id, viewport)) => {
                self.active_board_id = active_board_id;
                self.canvas.apply_viewport_state(&viewport);
                self.document_persistence
                    .record_saved_user_session(self.current_user_session());
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

    fn visible_canvas_screen_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let sidebar_rect = sidebar_rect(screen_rect);
        let left = if self.sidebar_open {
            sidebar_rect.right()
        } else {
            screen_rect.left()
        };
        egui::Rect::from_min_max(egui::pos2(left, screen_rect.top()), screen_rect.max)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn queue_import_placeholder_for_board(
        &mut self,
        board_id: i64,
        queue_id: u64,
        label: String,
        placeholder_world: egui::Pos2,
        width: u32,
        height: u32,
    ) {
        let placeholder = self.import_intake.queue_placeholder_for_board(
            board_id,
            queue_id,
            label,
            placeholder_world,
            width,
            height,
            self.settings.auto_image_scaling,
            self.settings.import_target_canvas_area,
        );
        if board_id == self.active_board_id {
            self.add_import_placeholder_to_canvas(placeholder);
        }
    }

    fn remove_import_placeholder_for_board(&mut self, board_id: i64, queue_id: u64) {
        self.import_intake
            .remove_placeholder_for_board(board_id, queue_id);
        if board_id == self.active_board_id {
            self.canvas.remove_import_placeholder(queue_id);
        }
    }

    fn restore_active_board_import_placeholders(&mut self) {
        for placeholder in self
            .import_intake
            .placeholders_for_board(self.active_board_id)
        {
            self.add_import_placeholder_to_canvas(placeholder);
        }
    }

    fn add_import_placeholder_to_canvas(
        &mut self,
        placeholder: crate::import_intake::CanvasImportPlaceholder,
    ) {
        self.canvas.add_import_placeholder(
            placeholder.queue_id,
            placeholder.center_world,
            placeholder.size_world,
            placeholder.label,
        );
    }

    fn import_size_world(&self, width: u32, height: u32) -> egui::Vec2 {
        crate::import_intake::import_preview_size_world(
            width,
            height,
            self.settings.import_target_canvas_area,
        )
    }

    fn import_placement_settings(&self) -> ImportPlacementSettings {
        ImportPlacementSettings {
            import_target_canvas_area: self.settings.import_target_canvas_area,
            import_gap: self.settings.import_gap,
            snap_distance_world: crate::import_placement::import_snap_distance_world(&self.canvas),
        }
    }

    fn import_request_config(&self) -> ImportRequestConfig {
        ImportRequestConfig {
            max_texture_side: self.max_texture_side,
            canvas_image_max_side: self.settings.canvas_image_max_side,
            auto_image_scaling: true,
            import_target_canvas_area: self.settings.import_target_canvas_area,
            import_gap: self.settings.import_gap,
        }
    }

    fn import_trace(&self) -> Option<crate::jobs::ImportTrace> {
        self.perf_log.as_ref().map(|_| crate::jobs::ImportTrace {
            queued_at: Instant::now(),
            timings: crate::jobs::ImportTimings::default(),
        })
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
        let size = self.import_size_world(width, height);
        crate::import_placement::reserve_import_world(
            layout,
            desired_world,
            size,
            self.settings.import_gap,
            crate::import_placement::import_snap_distance_world(&self.canvas),
        )
    }

    fn reserve_import(
        &mut self,
        layout: &mut Vec<crate::data::BoardLayoutItem>,
        label: String,
        dimensions: Option<(u32, u32)>,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
    ) -> crate::import_intake::ImportReservation {
        let _ = viewport_world;
        let (reservation, placeholder) = self.import_intake.reserve_import(
            layout,
            self.active_board_id,
            label,
            dimensions,
            desired_world,
            self.import_placement_settings(),
        );
        if let Some(placeholder) = placeholder {
            self.add_import_placeholder_to_canvas(placeholder);
        }
        reservation
    }

    fn remove_import_reservation(&mut self, reservation: crate::import_intake::ImportReservation) {
        self.import_intake.remove_import_reservation(reservation);
        if reservation.target_board_id == self.active_board_id {
            self.canvas.remove_import_placeholder(reservation.queue_id);
        }
    }

    fn normalize_import_url(raw: &str) -> Option<String> {
        crate::import_intake::normalize_import_url(raw)
    }

    fn import_source_label(source: &ExternalImportSource) -> String {
        crate::import_intake::import_source_label(source)
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
            let scale = crate::jobs::initial_import_scale(
                width,
                height,
                true,
                self.settings.import_target_canvas_area,
            );
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
            let label = Self::import_source_label(&source);
            let dimensions = match &source {
                ExternalImportSource::Path(path) => image::image_dimensions(path).ok(),
                ExternalImportSource::Url(_) => Some((FALLBACK_WIDTH, FALLBACK_HEIGHT)),
            };
            let reservation = self.reserve_import(
                &mut layout,
                label,
                dimensions,
                desired_world,
                viewport_world,
            );

            let request = match &source {
                ExternalImportSource::Path(source_path) => {
                    crate::import_intake::import_path_request(
                        source_path.clone(),
                        reservation,
                        desired_world,
                        viewport_world,
                        self.import_request_config(),
                        self.import_trace(),
                    )
                }
                ExternalImportSource::Url(url) => crate::import_intake::import_url_request(
                    url.clone(),
                    reservation,
                    desired_world,
                    viewport_world,
                    self.import_request_config(),
                    self.import_trace(),
                ),
            };

            if let Err(error) = self
                .import_worker
                .as_ref()
                .expect("checked import worker")
                .enqueue(request)
            {
                self.remove_import_reservation(reservation);
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

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        if self.handle_internal_clipboard_paste_at(&paste_text, desired_world) {
            return;
        }

        if self.handle_clipboard_url_import_at(screen_rect, &paste_text, desired_world) {
            return;
        }

        self.handle_external_clipboard_image_paste_at(screen_rect, desired_world);
    }

    fn handle_external_clipboard_image_paste_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) {
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

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let rgba = clipboard_image.bytes.into_owned();
        let width = clipboard_image.width as u32;
        let height = clipboard_image.height as u32;
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let reservation = self.reserve_import(
            &mut layout,
            "Clipboard".to_owned(),
            Some((width, height)),
            desired_world,
            viewport_world,
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(crate::import_intake::import_clipboard_request(
                rgba,
                width,
                height,
                reservation,
                desired_world,
                viewport_world,
                self.import_request_config(),
                self.import_trace(),
            ))
        {
            self.remove_import_reservation(reservation);
            self.status = error;
            return;
        }

        self.status = "Queued clipboard image".to_owned();
    }

    fn handle_clipboard_url_import_at(
        &mut self,
        screen_rect: egui::Rect,
        paste_text: &str,
        desired_world: egui::Pos2,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }
        let Some(url) = Self::normalize_import_url(paste_text) else {
            return false;
        };

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_layout_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return true;
            }
        };
        let reservation = self.reserve_import(
            &mut layout,
            Self::import_source_label(&ExternalImportSource::Url(url.clone())),
            Some((360, 240)),
            desired_world,
            viewport_world,
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(crate::import_intake::import_url_request(
                url.clone(),
                reservation,
                desired_world,
                viewport_world,
                self.import_request_config(),
                self.import_trace(),
            ))
        {
            self.remove_import_reservation(reservation);
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
        if !copy_requested || !self.canvas.has_selection() {
            return;
        }

        let outcome = match crate::active_board_commands::copy_selection_to_json(&self.canvas) {
            Ok(Some(outcome)) => outcome,
            Ok(None) => return,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        ctx.copy_text(outcome.payload_json);

        let noun = if outcome.item_count == 1 {
            "item"
        } else {
            "items"
        };
        self.status = format!("Copied {} {noun}", outcome.item_count);
    }

    fn copy_selection_to_context_clipboard(&mut self, ctx: &egui::Context) {
        if !self.canvas.has_selection() {
            return;
        }

        let outcome = match crate::active_board_commands::copy_selection_to_json(&self.canvas) {
            Ok(Some(outcome)) => outcome,
            Ok(None) => return,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        ctx.copy_text(outcome.payload_json);

        let noun = if outcome.item_count == 1 {
            "item"
        } else {
            "items"
        };
        self.status = format!("Copied {} {noun}", outcome.item_count);
    }

    fn context_menu_items(&self) -> Vec<ContextMenuItem> {
        let selected_item_count = self.canvas.selected_item_ids().len();
        let selected_annotation_count = self.canvas.selected_annotation_ids().len();
        let has_selection = selected_item_count > 0 || selected_annotation_count > 0;
        let has_selected_image = selected_item_count > 0;
        let exactly_one_image_only = selected_item_count == 1 && selected_annotation_count == 0;

        vec![
            ContextMenuItem {
                action: ContextMenuAction::Copy,
                label: "Copy",
                shortcut: Some("⌘ C"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::Paste,
                label: "Paste",
                shortcut: Some("⌘ V"),
                enabled: true,
            },
            ContextMenuItem {
                action: ContextMenuAction::Duplicate,
                label: "Duplicate",
                shortcut: Some("⌘ D"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::DeleteFromBoard,
                label: "Delete (from board)",
                shortcut: Some("del"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToFront,
                label: "Move to Front",
                shortcut: Some("⌘ ."),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToBack,
                label: "Move to Back",
                shortcut: Some("⌘ ,"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::SearchUsingImage,
                label: "Search using image",
                shortcut: Some("⌘ F"),
                enabled: exactly_one_image_only,
            },
            ContextMenuItem {
                action: ContextMenuAction::RemoveFromLibrary,
                label: "Remove from Library...",
                shortcut: None,
                enabled: has_selected_image,
            },
        ]
    }

    fn handle_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let Some(item) = self
            .context_menu_items()
            .into_iter()
            .find(|item| item.action == action)
        else {
            return;
        };
        if !item.enabled {
            return;
        }

        match action {
            ContextMenuAction::Copy => self.copy_selection_to_context_clipboard(ctx),
            ContextMenuAction::Paste => self.handle_clipboard_paste_from_context(ctx, screen_rect),
            ContextMenuAction::Duplicate => self.duplicate_selection_with_gap(),
            ContextMenuAction::DeleteFromBoard => {
                self.delete_selected_items_from_board(screen_rect);
            }
            ContextMenuAction::MoveToFront => self.move_selected_canvas_items_to_front(),
            ContextMenuAction::MoveToBack => self.move_selected_canvas_items_to_back(),
            ContextMenuAction::SearchUsingImage => self.search_using_selected_image(),
            ContextMenuAction::RemoveFromLibrary => self.request_selected_assets_library_remove(),
        }
    }

    fn move_selected_canvas_items_to_front(&mut self) {
        let viewport_world = self.startup_viewport_world_rect();
        let mut context = self.active_board_command_context(viewport_world);
        if let Err(error) = crate::active_board_commands::move_selected_to_front(&mut context) {
            self.status = error;
        }
    }

    fn move_selected_canvas_items_to_back(&mut self) {
        let viewport_world = self.startup_viewport_world_rect();
        let mut context = self.active_board_command_context(viewport_world);
        if let Err(error) = crate::active_board_commands::move_selected_to_back(&mut context) {
            self.status = error;
        }
    }

    fn handle_clipboard_paste_from_context(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let paste_text = self.context_clipboard_text(ctx);
        if let Some(paste_text) = paste_text {
            let desired_world = self
                .context_menu_pos
                .map(|pos| self.canvas.screen_to_world(pos, screen_rect))
                .unwrap_or_else(|| self.preferred_import_world(ctx, screen_rect));
            if self.handle_internal_clipboard_paste_at(&paste_text, desired_world) {
                return;
            }
            if self.handle_clipboard_url_import_at(screen_rect, &paste_text, desired_world) {
                return;
            }
        }

        let desired_world = self
            .context_menu_pos
            .map(|pos| self.canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| self.preferred_import_world(ctx, screen_rect));
        self.handle_external_clipboard_image_paste_at(screen_rect, desired_world);
    }

    fn context_clipboard_text(&mut self, ctx: &egui::Context) -> Option<String> {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        if paste_text.is_some() {
            return paste_text;
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(_) => return None,
        };
        clipboard.get_text().ok()
    }

    fn request_selected_assets_library_remove(&mut self) {
        let mut asset_ids = Vec::new();
        for item_id in self.canvas.selected_item_ids() {
            if let Some(record) = self.canvas.record_for_item_id(item_id)
                && !asset_ids.contains(&record.asset.id)
            {
                asset_ids.push(record.asset.id);
            }
        }

        if asset_ids.is_empty() {
            return;
        }

        self.pending_library_remove = Some(PendingLibraryRemove { asset_ids });
    }

    fn confirm_pending_library_remove(&mut self) {
        let Some(pending) = self.pending_library_remove.take() else {
            return;
        };

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        if let Err(error) = library.archive_assets(&pending.asset_ids) {
            self.status = error;
            return;
        }

        self.canvas.remove_assets(&pending.asset_ids);
        let noun = if pending.asset_ids.len() == 1 {
            "asset"
        } else {
            "assets"
        };
        self.status = format!("Removed {} library {noun}", pending.asset_ids.len());
    }

    fn show_context_menu(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        just_opened: bool,
    ) {
        let Some(pos) = self.context_menu_pos else {
            return;
        };

        let mut close_menu = false;
        let menu_response = egui::Area::new(egui::Id::new("board_context_menu"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(250.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for item in self.context_menu_items() {
                        let response = context_menu_row(ui, &item);
                        if response.clicked() {
                            self.handle_context_menu_action(item.action, ctx, screen_rect);
                            close_menu = true;
                        }
                    }
                });
            });

        let should_close = ctx.input(|input| {
            context_menu_should_close(
                close_menu,
                just_opened,
                menu_response.response.rect,
                input.pointer.latest_pos(),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if should_close {
            self.context_menu_pos = None;
        }
    }

    fn blocks_canvas_input(&self, ctx: &egui::Context) -> bool {
        self.context_menu_pos.is_some()
            || self.pending_board_delete.is_some()
            || self.pending_library_remove.is_some()
            || {
                #[cfg(feature = "semantic-search")]
                {
                    self.search.blocks_canvas_input(ctx)
                }
                #[cfg(not(feature = "semantic-search"))]
                {
                    let _ = ctx;
                    false
                }
            }
    }

    fn show_library_remove_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_library_remove.clone() else {
            return;
        };

        let title = if pending.asset_ids.len() == 1 {
            "Remove asset from Library?"
        } else {
            "Remove assets from Library?"
        };
        egui::Window::new(title)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(destructive_confirmation_frame(ctx))
            .show(ctx, |ui| {
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, title);
                destructive_dialog_body(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Placements will be removed from all boards. Original files are kept.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.pending_library_remove = None;
                        }
                        if ui
                            .add(destructive_action_button("Remove from Library"))
                            .clicked()
                        {
                            self.confirm_pending_library_remove();
                        }
                    });
                });
            });
    }

    #[cfg(feature = "semantic-search")]
    fn search_using_selected_image(&mut self) {
        self.search.search_using_selected_image(
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
        );
    }

    #[cfg(not(feature = "semantic-search"))]
    fn search_using_selected_image(&mut self) {
        self.status = "Semantic search unavailable".to_owned();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn copy_selected_board_payload(&self) -> Option<ClipboardBoardSelection> {
        crate::active_board_commands::copy_selected_board_payload(&self.canvas)
    }

    fn handle_internal_clipboard_paste_at(
        &mut self,
        paste_text: &str,
        anchor_world: egui::Pos2,
    ) -> bool {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board_commands::paste_json_at(&mut context, paste_text, anchor_world) {
            Ok(Some(outcome)) => {
                self.apply_paste_command_outcome(outcome);
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.status = error;
                true
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn paste_board_payload_at(
        &mut self,
        payload: &ClipboardBoardSelection,
        requested_anchor_world: egui::Pos2,
    ) -> Result<(), String> {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let mut context = self.active_board_command_context(viewport_world);
        let outcome = crate::active_board_commands::paste_board_payload_at(
            &mut context,
            payload,
            requested_anchor_world,
        )?;
        self.apply_paste_command_outcome(outcome);
        Ok(())
    }

    fn apply_paste_command_outcome(&mut self, outcome: PasteCommandOutcome) {
        let needs_existing_reload = outcome.needs_existing_reload;
        self.status = outcome.status;
        if needs_existing_reload {
            self.enqueue_existing_items();
        }
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
            let outcome = self.residency.process_import_result(
                ctx,
                &mut self.canvas,
                self.import_worker.as_ref(),
                self.active_board_id,
                &self.settings,
                result,
            );
            self.apply_import_result_residency_outcome(ctx, outcome);
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

    fn apply_import_result_residency_outcome(
        &mut self,
        ctx: &egui::Context,
        outcome: ImportResultResidencyOutcome,
    ) {
        match outcome {
            ImportResultResidencyOutcome::Loaded(outcome) => {
                self.apply_loaded_import_outcome(outcome);
            }
            ImportResultResidencyOutcome::Cropped(cropped) => {
                self.integrate_cropped_import_result(ctx, cropped);
            }
            ImportResultResidencyOutcome::Failed(outcome) => {
                self.apply_failed_import_outcome(outcome);
            }
            ImportResultResidencyOutcome::Status(Some(status)) => {
                self.status = status;
            }
            ImportResultResidencyOutcome::Status(None) => {}
        }
    }

    fn integrate_cropped_import_result(
        &mut self,
        ctx: &egui::Context,
        cropped: crate::jobs::CroppedCanvasItem,
    ) {
        match self.crop_flow.integrate_cropped_result(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            cropped,
            self.settings.canvas_image_max_side,
            self.settings.semantic_search_enabled,
        ) {
            Ok(integration) => {
                self.residency
                    .note_loaded_item(integration.updated_record.item.id);
                #[cfg(feature = "semantic-search")]
                if let Some((asset_id, original_path)) = integration.indexed_asset {
                    self.search.queue_asset_index(asset_id, original_path);
                }
                if let Some(error) = self.residency.queue_overview_backfill_after_working_load(
                    self.import_worker.as_ref(),
                    &integration.updated_record,
                    &self.settings,
                    crate::data::CanvasImageCacheTier::Working,
                ) {
                    self.status = error;
                }
                self.status = "Cropped image".to_owned();
            }
            Err(error) => self.status = error,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn integrate_loaded_import(
        &mut self,
        ctx: &egui::Context,
        loaded: crate::jobs::LoadedCanvasItem,
    ) {
        let outcome = self.residency.integrate_loaded_import(
            ctx,
            &mut self.canvas,
            self.import_worker.as_ref(),
            self.active_board_id,
            &self.settings,
            loaded,
        );
        self.apply_loaded_import_outcome(outcome);
    }

    fn apply_loaded_import_outcome(&mut self, outcome: LoadedImportOutcome) {
        if let Some(placeholder) = outcome.placeholder_removal {
            self.remove_import_placeholder_for_board(
                placeholder.target_board_id,
                placeholder.queue_id,
            );
        }
        #[cfg(feature = "semantic-search")]
        if let Some((asset_id, original_path)) = outcome.indexed_asset {
            self.search.queue_asset_index(asset_id, original_path);
        }
        if let Some(log) = outcome.log {
            self.log_loaded_timing(
                log.origin,
                Some(&log.record),
                log.diagnostics.as_ref(),
                log.trace,
            );
        }
        if let Some(status) = outcome.status {
            self.status = status;
        }
    }

    fn apply_failed_import_outcome(&mut self, outcome: FailedImportOutcome) {
        if let Some(placeholder) = outcome.placeholder_removal {
            self.remove_import_placeholder_for_board(
                placeholder.target_board_id,
                placeholder.queue_id,
            );
        }
        self.status = outcome.status;
    }

    fn handle_crop_undo(&mut self, ctx: &egui::Context) {
        let undo_requested = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        if !undo_requested {
            return;
        }

        match self.crop_flow.undo_last_crop(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            self.active_board_id,
        ) {
            Ok(true) => self.status = "Undid crop".to_owned(),
            Ok(false) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_selection_shortcuts(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        #[cfg(feature = "semantic-search")]
        if self.search.is_open() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let select_all_requested =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A));
        if select_all_requested {
            self.canvas.select_all_content();
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
                | SupportedShortcutAction::MoveSelectionToFront
                | SupportedShortcutAction::MoveSelectionToBack
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
                    self.delete_selected_items_from_board(screen_rect);
                }
                SupportedShortcutAction::DuplicateSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.duplicate_selection_with_gap();
                }
                SupportedShortcutAction::MoveSelectionToFront => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.move_selected_canvas_items_to_front();
                }
                SupportedShortcutAction::MoveSelectionToBack => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.move_selected_canvas_items_to_back();
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

    fn handle_board_shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending_board_delete.is_some() || self.pending_library_remove.is_some() {
            return;
        }

        #[cfg(feature = "semantic-search")]
        if self.search.is_open() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let create_requested =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::N));
        if create_requested {
            self.create_and_switch_board(false);
            return;
        }

        let requested_index = ctx.input(|input| {
            if !input.modifiers.command {
                return None;
            }
            [
                (egui::Key::Num1, 0_usize),
                (egui::Key::Num2, 1),
                (egui::Key::Num3, 2),
                (egui::Key::Num4, 3),
                (egui::Key::Num5, 4),
                (egui::Key::Num6, 5),
                (egui::Key::Num7, 6),
                (egui::Key::Num8, 7),
                (egui::Key::Num9, 8),
                (egui::Key::Num0, 9),
            ]
            .into_iter()
            .find_map(|(key, index)| input.key_pressed(key).then_some(index))
        });

        let Some(index) = requested_index else {
            return;
        };
        let boards = self.board_summaries();
        if let Some(summary) = boards.get(index) {
            self.switch_to_board(summary.board.id);
        }
    }

    fn duplicate_selection_with_gap(&mut self) {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board_commands::duplicate_selection(&mut context) {
            Ok(Some(outcome)) => self.apply_paste_command_outcome(outcome),
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_pending_crop_requests(&mut self) {
        match self.crop_flow.enqueue_pending_requests(
            &mut self.canvas,
            self.import_worker.as_ref(),
            self.max_texture_side,
            self.settings.canvas_image_max_side,
        ) {
            Ok(true) => self.status = "Cropping image".to_owned(),
            Ok(false) => {}
            Err(error) => self.status = error,
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
        let session = self.current_user_session();
        if let Err(error) = self.document_persistence.commit_canvas_changes(
            self.library.as_ref(),
            &mut self.canvas,
            session,
        ) {
            self.status = error;
        }
    }

    fn current_user_session(&self) -> crate::data::UserSession {
        crate::data::UserSession {
            active_board_id: self.active_board_id,
            viewport: self.canvas.viewport_state(),
            settings: self.settings.clone(),
            sidebar_open: self.sidebar_open,
            sidebar_tab: sidebar_tab_session_value(self.sidebar_tab).to_owned(),
        }
    }

    fn active_board_command_context(
        &mut self,
        viewport_world: egui::Rect,
    ) -> ActiveBoardCommandContext<'_> {
        let user_session = self.current_user_session();
        ActiveBoardCommandContext {
            canvas: &mut self.canvas,
            library: self.library.as_ref(),
            document_persistence: &mut self.document_persistence,
            user_session,
            import_worker_available: self.import_worker.is_some(),
            import_gap: self.settings.import_gap,
            viewport_world,
        }
    }

    fn delete_selected_items_from_board(&mut self, screen_rect: egui::Rect) {
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board_commands::delete_selected_from_board(&mut context) {
            Ok(Some(status)) => self.status = status,
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn save_user_session(&mut self) {
        let session = self.current_user_session();
        if let Err(error) = self
            .document_persistence
            .save_user_session(self.library.as_ref(), session)
        {
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
                target_board_id: self.active_board_id,
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
                target_board_id: self.active_board_id,
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
    fn seed_svg_verification(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        self.canvas.clear_scene_for_verification();
        let width = 240_u32;
        let height = 160_u32;
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="{width}" height="{height}" rx="0" fill="#f8fafc"/><rect x="18" y="18" width="204" height="124" fill="#14b8a6"/><circle cx="120" cy="80" r="46" fill="#2563eb"/><path d="M52 116 L102 54 L139 98 L164 70 L204 116 Z" fill="#f59e0b"/></svg>"##
        )
        .into_bytes();
        let asset = crate::data::ImageAsset {
            id: -60,
            original_path: std::path::PathBuf::from("/tmp/debug.svg"),
            filename: "debug.svg".to_owned(),
            mime_type: "image/svg+xml".to_owned(),
            width,
            height,
            file_size: svg.len() as u64,
            imported_at: 0,
        };
        let record = crate::data::BoardItemWithAsset {
            item: crate::data::BoardItem {
                id: -60,
                asset_id: -60,
                x: 0.0,
                y: 0.0,
                scale: 1.5,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset,
        };
        self.canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                target_board_id: self.active_board_id,
                queue_id: None,
                record,
                preview_rgba: Vec::new(),
                preview_width: width,
                preview_height: height,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Svg {
                    uri: "bytes://verify-svg.svg".to_owned(),
                    bytes: std::sync::Arc::<[u8]>::from(svg),
                    size: [width as usize, height as usize],
                },
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        self.canvas.select_items(&[-60]);
        self.status = "Added SVG debug image".to_owned();
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
    fn seed_context_menu_verification(&mut self, ctx: &egui::Context) {
        self.canvas
            .apply_viewport_state(&crate::data::ViewportState::default());
        self.canvas.clear_scene_for_verification();

        let image_result = self.seed_import_verification_image(
            ctx,
            "verify-context-menu",
            egui::pos2(-80.0, 0.0),
            180,
            140,
            [70, 142, 255, 255],
        );
        let annotation_result = self.library.as_ref().map(|library| {
            library.create_canvas_annotation(
                crate::data::CanvasAnnotationKind::Textbox,
                egui::pos2(80.0, -60.0),
                egui::pos2(260.0, 20.0),
                "menu target",
                crate::data::CanvasTextSize::M,
                crate::data::CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Gold,
                true,
            )
        });

        match (image_result, annotation_result) {
            (Ok(item_id), Some(Ok(annotation))) => {
                let annotation_id = annotation.id;
                self.canvas.add_annotation(annotation);
                self.canvas.select_content(&[item_id], &[annotation_id]);
                self.context_menu_pos = Some(egui::pos2(520.0, 260.0));
                self.status = "Opened debug context menu".to_owned();
            }
            (Err(error), _) | (_, Some(Err(error))) => self.status = error,
            (_, None) => self.status = "Library unavailable".to_owned(),
        }
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn seed_search_latency_probe(&mut self) {
        self.settings.semantic_search_enabled = true;
        self.sync_semantic_search_service();
        self.search_latency_probe = Some(SearchLatencyProbe {
            phase: SearchLatencyProbePhase::WaitExistingImage,
            selected_item_id: None,
            probe_started_at: Instant::now(),
            command_started_at: None,
            tab_started_at: None,
            board_results_ms: None,
            board_result_ready_ms: None,
            library_result_ready_ms: None,
        });
        eprintln!("[search-latency] probe started; waiting for first existing canvas image");
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
            let _reservation = self.reserve_import(
                &mut layout,
                label.to_owned(),
                Some((width, height)),
                desired_world,
                viewport_world,
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
            let reservation = self.reserve_import(
                &mut layout,
                label.to_owned(),
                Some((width, height)),
                desired_world,
                viewport_world,
            );
            let rgba = Self::solid_rgba(width, height, color);
            let enqueue_result = self.import_worker.as_ref().and_then(|import_worker| {
                import_worker
                    .enqueue(crate::import_intake::import_clipboard_request(
                        rgba,
                        width,
                        height,
                        reservation,
                        desired_world,
                        viewport_world,
                        self.import_request_config(),
                        None,
                    ))
                    .err()
            });
            if let Some(error) = enqueue_result {
                self.remove_import_reservation(reservation);
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
            self.verification_search_scope,
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

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn drive_search_latency_probe(&mut self, ctx: &egui::Context) -> bool {
        let Some(mut probe) = self.search_latency_probe.take() else {
            return false;
        };

        match probe.phase {
            SearchLatencyProbePhase::WaitExistingImage => {
                let item_ids = self.canvas.all_item_ids();
                let preferred = item_ids.iter().copied().find(|item_id| {
                    self.canvas
                        .record_for_item_id(*item_id)
                        .is_some_and(|record| record.asset.id == 179)
                });
                if let Some(item_id) = preferred.or_else(|| item_ids.first().copied()) {
                    let wait_ms = probe.probe_started_at.elapsed().as_secs_f64() * 1000.0;
                    let asset_id = self
                        .canvas
                        .record_for_item_id(item_id)
                        .map(|record| record.asset.id)
                        .unwrap_or_default();
                    probe.selected_item_id = Some(item_id);
                    self.canvas.select_items(&[item_id]);
                    eprintln!(
                        "[search-latency] selected_existing_item_id={item_id} selected_asset_id={asset_id} wait_for_item_ms={wait_ms:.2}"
                    );
                    probe.phase = SearchLatencyProbePhase::OpenSearch;
                }
            }
            SearchLatencyProbePhase::OpenSearch => {
                let Some(selected_item_id) = probe.selected_item_id else {
                    probe.phase = SearchLatencyProbePhase::WaitExistingImage;
                    self.search_latency_probe = Some(probe);
                    return true;
                };
                self.canvas.select_items(&[selected_item_id]);
                let started_at = Instant::now();
                self.search_using_selected_image();
                probe.command_started_at = Some(started_at);
                probe.phase = SearchLatencyProbePhase::WaitBoardResults;
                eprintln!("[search-latency] command_f_opened_image_search");
            }
            SearchLatencyProbePhase::WaitBoardResults => {
                if self.search.verification_result_count() > 0
                    && self.search.verification_scope()
                        == crate::semantic_search::SearchScope::ThisBoard
                {
                    let ready_ms = probe
                        .command_started_at
                        .map(|started_at| started_at.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    probe.board_result_ready_ms = Some(ready_ms);
                    eprintln!("[search-latency] board_result_ready_ms={ready_ms:.2}");
                    probe.phase = SearchLatencyProbePhase::WaitBoardPaint;
                }
            }
            SearchLatencyProbePhase::WaitBoardPaint => {
                if self.search.verification_result_count() > 0
                    && self.search.verification_scope()
                        == crate::semantic_search::SearchScope::ThisBoard
                {
                    let board_ms = probe
                        .command_started_at
                        .map(|started_at| started_at.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    eprintln!("[search-latency] board_results_ms={board_ms:.2}");
                    probe.board_results_ms = Some(board_ms);
                    probe.phase = SearchLatencyProbePhase::SwitchToLibrary;
                }
            }
            SearchLatencyProbePhase::SwitchToLibrary => {
                let started_at = Instant::now();
                self.search.verification_toggle_scope(&mut self.canvas);
                probe.tab_started_at = Some(started_at);
                probe.phase = SearchLatencyProbePhase::WaitLibraryResults;
                eprintln!("[search-latency] tab_pressed_switch_to_library");
            }
            SearchLatencyProbePhase::WaitLibraryResults => {
                if self.search.verification_result_count() > 0
                    && self.search.verification_scope()
                        == crate::semantic_search::SearchScope::Library
                {
                    let ready_ms = probe
                        .tab_started_at
                        .map(|started_at| started_at.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    probe.library_result_ready_ms = Some(ready_ms);
                    eprintln!("[search-latency] library_result_ready_ms={ready_ms:.2}");
                    probe.phase = SearchLatencyProbePhase::WaitLibraryPaint;
                }
            }
            SearchLatencyProbePhase::WaitLibraryPaint => {
                if self.search.verification_result_count() > 0
                    && self.search.verification_scope()
                        == crate::semantic_search::SearchScope::Library
                {
                    let library_ms = probe
                        .tab_started_at
                        .map(|started_at| started_at.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    let board_ms = probe.board_results_ms.unwrap_or_default();
                    let board_ready_ms = probe.board_result_ready_ms.unwrap_or_default();
                    let library_ready_ms = probe.library_result_ready_ms.unwrap_or_default();
                    eprintln!(
                        "[search-latency] summary board_ready_ms={board_ready_ms:.2} board_painted_ms={board_ms:.2} library_ready_ms={library_ready_ms:.2} library_painted_ms={library_ms:.2}"
                    );
                    probe.phase = SearchLatencyProbePhase::Done;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            SearchLatencyProbePhase::Done => {}
        }

        let active = probe.phase != SearchLatencyProbePhase::Done;
        self.search_latency_probe = Some(probe);
        if active {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        active
    }

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn drive_verification_search(&mut self) {}

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn verification_search_settled(&self) -> bool {
        true
    }

    #[cfg(not(all(debug_assertions, feature = "semantic-search")))]
    fn drive_search_latency_probe(&mut self, _ctx: &egui::Context) -> bool {
        false
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
        self.search.handle_results(&mut self.canvas);
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

fn sidebar_tab_session_value(tab: SidebarTab) -> &'static str {
    match tab {
        SidebarTab::Boards => "boards",
        SidebarTab::Options => "settings",
    }
}

fn sidebar_tab_from_session(value: &str) -> SidebarTab {
    match value {
        "boards" => SidebarTab::Boards,
        _ => SidebarTab::Options,
    }
}

fn context_menu_should_close(
    action_clicked: bool,
    just_opened: bool,
    menu_rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
    primary_pressed: bool,
    secondary_pressed: bool,
    escape_pressed: bool,
) -> bool {
    if action_clicked || escape_pressed {
        return true;
    }
    if just_opened || (!primary_pressed && !secondary_pressed) {
        return false;
    }
    pointer_pos.is_none_or(|pos| !menu_rect.contains(pos))
}

fn context_menu_row(ui: &mut egui::Ui, item: &ContextMenuItem) -> egui::Response {
    let row_height = 28.0;
    let shortcut_width: f32 = 54.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        if item.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );

    let rounding = egui::CornerRadius::same(4);
    if response.hovered() && item.enabled {
        ui.painter()
            .rect_filled(rect, rounding, egui::Color32::from_white_alpha(18));
    }

    let text_color = if item.enabled && item.action.is_destructive() {
        crate::theme::DESTRUCTIVE
    } else if item.enabled {
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let shortcut_color = if item.enabled {
        crate::theme::TEXT_SUBTLE
    } else {
        crate::theme::TEXT_MUTED
    };

    let label_pos = egui::pos2(rect.left() + 8.0, rect.center().y - 7.5);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        item.label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    if let Some(shortcut) = item.shortcut {
        let pill_size = egui::vec2(shortcut_width.min(rect.width() * 0.35), 20.0);
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 8.0 - pill_size.x * 0.5, rect.center().y),
            pill_size,
        );
        ui.painter().rect_filled(
            pill_rect,
            egui::CornerRadius::same(4),
            egui::Color32::from_white_alpha(12),
        );
        ui.painter().rect_stroke(
            pill_rect,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(16)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            shortcut,
            egui::FontId::monospace(11.0),
            shortcut_color,
        );
    }

    response
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
        self.drive_search_latency_probe(ui.ctx());
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

        #[cfg(debug_assertions)]
        if matches!(
            self.verification_mode,
            Some(
                VerificationMode::DesignSystemPreview
                    | VerificationMode::CompactControlsComparison
                    | VerificationMode::RadiusComparison
                    | VerificationMode::DestructiveActionsComparison
                    | VerificationMode::SpacingComparison
                    | VerificationMode::BoardRowsComparison
                    | VerificationMode::BoardRowsComparisonBottom
            )
        ) {
            match self.verification_mode {
                Some(VerificationMode::CompactControlsComparison) => {
                    crate::design_system_preview::show_compact_controls_comparison(ui, screen_rect);
                }
                Some(VerificationMode::RadiusComparison) => {
                    crate::design_system_preview::show_radius_comparison(ui, screen_rect);
                }
                Some(VerificationMode::DestructiveActionsComparison) => {
                    crate::design_system_preview::show_destructive_actions_comparison(
                        ui,
                        screen_rect,
                    );
                }
                Some(VerificationMode::SpacingComparison) => {
                    crate::design_system_preview::show_spacing_comparison(ui, screen_rect);
                }
                Some(VerificationMode::BoardRowsComparison) => {
                    crate::design_system_preview::show_board_rows_comparison(ui, screen_rect, 0.0);
                }
                Some(VerificationMode::BoardRowsComparisonBottom) => {
                    crate::design_system_preview::show_board_rows_comparison(
                        ui,
                        screen_rect,
                        560.0,
                    );
                }
                _ => crate::design_system_preview::show(ui, screen_rect),
            }
            if self
                .screenshot_capture
                .handle_event(ui.ctx(), &mut self.status)
            {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        self.handle_clipboard_copy(ui.ctx());
        self.handle_clipboard_paste(ui.ctx(), screen_rect);
        self.handle_hovered_files(ui, screen_rect);
        self.handle_dropped_files(ui, screen_rect);
        #[cfg(feature = "semantic-search")]
        self.handle_search_shortcut(ui.ctx());
        self.handle_board_shortcuts(ui.ctx());
        self.handle_selection_shortcuts(ui.ctx(), screen_rect);
        let sidebar_open = self.sidebar_open;
        if self.should_block_sidebar_input() {
            block_sidebar_input(ui.ctx(), sidebar_rect(screen_rect));
        }
        self.canvas
            .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
        self.canvas.set_layout_gap(self.settings.import_gap);
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        let suppress_canvas_input = self.blocks_canvas_input(ui.ctx());
        self.frame_perf.canvas = crate::canvas::show(
            ui,
            screen_rect,
            visible_canvas_rect,
            &mut self.canvas,
            self.wgpu_render_state.as_ref(),
            true,
            suppress_canvas_input,
            false,
        );
        let context_menu_just_opened =
            if let Some(request) = self.canvas.take_context_menu_request() {
                self.context_menu_pos = Some(request.screen_pos);
                true
            } else {
                false
            };
        if let Some(error) = self.residency.dispatch_pending_tier_requests(
            self.import_worker.as_ref(),
            &mut self.canvas,
            self.active_board_id,
            &self.settings,
            self.max_texture_side,
        ) {
            self.status = error;
        }
        self.handle_pending_crop_requests();
        self.handle_pending_annotation_requests();
        self.persist_canvas_changes();
        let board_summaries = self.board_summaries();
        let active_board_name = board_summaries
            .iter()
            .find(|summary| summary.board.id == self.active_board_id)
            .map(|summary| summary.board.name.clone())
            .unwrap_or_else(|| "My Board".to_owned());
        let sidebar_result = show_sidebar(
            ui.ctx(),
            screen_rect,
            sidebar_open,
            &mut self.sidebar_tab,
            &mut self.settings,
            &mut self.canvas,
            &board_summaries,
            self.active_board_id,
            #[cfg(debug_assertions)]
            matches!(
                self.verification_mode,
                Some(VerificationMode::OpenSidebarBoardsHover)
            )
            .then_some(1),
            #[cfg(not(debug_assertions))]
            None,
        );
        self.handle_board_action(sidebar_result.board_action);
        let modal_open =
            self.pending_board_delete.is_some() || self.pending_library_remove.is_some();
        if sidebar_open && !modal_open {
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
        self.show_board_delete_confirmation(ui.ctx());
        self.show_library_remove_confirmation(ui.ctx());
        if !modal_open {
            self.show_context_menu(ui.ctx(), screen_rect, context_menu_just_opened);
            if show_sidebar_hint(ui.ctx(), screen_rect, sidebar_open, &active_board_name) {
                self.sidebar_open = !self.sidebar_open;
            }
        }
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

impl crate::board_switch::BoardSwitchHost for ReferenceApp {
    fn active_board_id(&self) -> i64 {
        self.active_board_id
    }

    fn persist_active_board_before_switch(&mut self) {
        self.save_active_board_before_transition();
    }

    fn set_active_board_in_library(&mut self, board_id: i64) -> Result<(), String> {
        let Some(library) = &self.library else {
            return Err("Storage unavailable".to_owned());
        };
        library.set_active_board(board_id)
    }

    fn load_active_board(&mut self) {
        ReferenceApp::load_active_board(self);
    }
}

impl crate::active_board_load::ActiveBoardLoadHost for ReferenceApp {
    fn clear_board_local_transients(&mut self) {
        self.clear_active_board_transients();
    }

    fn clear_canvas_for_active_board_load(&mut self) {
        self.canvas.clear_for_board_switch();
    }

    fn load_active_board_viewport(&mut self) {
        ReferenceApp::load_session_viewport(self);
    }

    fn restore_active_board_import_placeholders(&mut self) {
        ReferenceApp::restore_active_board_import_placeholders(self);
    }

    fn enqueue_active_board_existing_items(&mut self) {
        ReferenceApp::enqueue_existing_items(self);
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

fn destructive_confirmation_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.global_style())
        .fill(crate::theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, crate::theme::DESTRUCTIVE_STROKE))
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_PANEL))
        .inner_margin(egui::Margin::same(0))
}

fn destructive_dialog_width() -> f32 {
    340.0
}

fn destructive_dialog_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(crate::theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.add_space(crate::theme::SPACE_3);
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Heading)
                .color(crate::theme::TEXT),
        );
    });
    ui.add_space(crate::theme::SPACE_2);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        crate::theme::stroke_subtle(),
    );
}

fn destructive_dialog_body(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_3 as i8,
            right: crate::theme::SPACE_3 as i8,
            top: crate::theme::SPACE_3 as i8,
            bottom: crate::theme::SPACE_3 as i8,
        })
        .show(ui, |ui| {
            add_contents(ui);
        });
}

fn destructive_action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(crate::theme::TEXT),
    )
    .fill(crate::theme::DESTRUCTIVE_FILL)
    .stroke(egui::Stroke::new(1.0, crate::theme::DESTRUCTIVE_STROKE))
    .corner_radius(crate::theme::RADIUS_INLINE)
}

fn secondary_dialog_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(crate::theme::TEXT),
    )
    .fill(crate::theme::SURFACE_ELEVATED)
    .stroke(crate::theme::stroke_subtle())
    .corner_radius(crate::theme::RADIUS_INLINE)
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

fn generated_board_name(boards: &[crate::data::Board]) -> String {
    let names = boards
        .iter()
        .filter(|board| !board.archived)
        .map(|board| board.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    (2..)
        .map(|number| format!("Board {number}"))
        .find(|name| !names.contains(name.as_str()))
        .unwrap_or_else(|| "Board 2".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{ArrangementAction, ReferenceApp, apply_arrangement_action};
    use crate::crop_flow::CropUndoEntry;
    use crate::data::{
        BoardItem, BoardItemWithAsset, CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth,
        CanvasTextSize, ImageAsset,
    };
    use crate::jobs::LoadedCanvasItem;
    #[cfg(feature = "semantic-search")]
    use crate::palette::{
        PALETTE_WIDTH, search_result_grid_content_height, search_result_grid_layout,
        search_result_tile_height,
    };
    #[cfg(feature = "semantic-search")]
    use crate::semantic_search::SearchScope;
    use crate::sidebar::{
        SidebarTab, block_sidebar_input, should_close_sidebar, show_sidebar, sidebar_rect,
    };
    use crate::storage::Library;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};
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
        app.settings.import_target_canvas_area = 45_000.0;
        app.settings.canvas_image_max_side = 4096;
        app.canvas
            .set_background(crate::data::CanvasBackground::Grid);

        app.save_user_session();

        let session = library.load_user_session().unwrap();
        assert!(!session.settings.auto_image_scaling);
        assert_eq!(session.settings.import_gap, 37.0);
        assert_eq!(session.settings.import_target_canvas_area, 45_000.0);
        assert_eq!(session.settings.canvas_image_max_side, 4096);
        assert_eq!(
            session.viewport.canvas_background,
            crate::data::CanvasBackground::Grid
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn save_user_session_round_trips_sidebar_state() {
        let root = temp_library_root("reference-app-sidebar-session");
        let library = Library::initialize(root.clone()).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.sidebar_open = true;
        app.sidebar_tab = SidebarTab::Boards;

        app.save_user_session();

        let mut reopened = test_app(Some(library));
        reopened.load_user_session();
        assert!(reopened.sidebar_open);
        assert_eq!(reopened.sidebar_tab, SidebarTab::Boards);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persist_canvas_changes_saves_viewport_changes() {
        let root = temp_library_root("reference-app-viewport-session");
        let library = Library::initialize(root.clone()).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 10.0,
                camera_pan_y: 20.0,
                camera_zoom: 1.75,
                canvas_background: crate::data::CanvasBackground::Dots,
            });

        app.persist_canvas_changes();

        let session = library.load_user_session().unwrap();
        assert_eq!(session.viewport.camera_pan_x, 10.0);
        assert_eq!(session.viewport.camera_pan_y, 20.0);
        assert_eq!(session.viewport.camera_zoom, 1.75);
        assert_eq!(
            session.viewport.canvas_background,
            crate::data::CanvasBackground::Dots
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
                    target_board_id: 1,
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

    #[test]
    fn cross_board_paste_creates_new_images_and_annotations_on_active_board() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-cross-board-paste");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let source_item = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let source_annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(40.0, 60.0),
                egui::pos2(140.0, 100.0),
                "note",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Sky,
                true,
            )
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: first.id,
                queue_id: None,
                record: source_item.clone(),
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        app.canvas.add_annotation(source_annotation.clone());
        let source_stack_order = vec![
            CanvasStackItem::Annotation(source_annotation.id),
            CanvasStackItem::Placement(source_item.item.id),
        ];
        app.canvas
            .set_canvas_stack_order(source_stack_order.clone());
        library
            .save_canvas_stack_order(&source_stack_order)
            .unwrap();
        app.canvas
            .select_content(&[source_item.item.id], &[source_annotation.id]);

        let payload = app.copy_selected_board_payload().unwrap();
        library.set_active_board(second.id).unwrap();
        app.active_board_id = second.id;
        app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
            .unwrap();

        let pasted_items = library.load_board_items_for_board(second.id).unwrap();
        let pasted_annotations = library.load_canvas_annotations().unwrap();
        assert_eq!(pasted_items.len(), 1);
        assert_eq!(pasted_annotations.len(), 1);
        assert_ne!(pasted_items[0].item.id, source_item.item.id);
        assert_ne!(pasted_annotations[0].id, source_annotation.id);
        assert_eq!(
            library.load_board_items_for_board(first.id).unwrap().len(),
            1
        );

        let pasted_delta = egui::vec2(
            pasted_annotations[0].start_x - pasted_items[0].item.x,
            pasted_annotations[0].start_y - pasted_items[0].item.y,
        );
        let source_delta = egui::vec2(
            source_annotation.start_x - source_item.item.x,
            source_annotation.start_y - source_item.item.y,
        );
        assert_eq!(pasted_delta, source_delta);
        assert_eq!(
            app.canvas.selected_item_ids(),
            vec![pasted_items[0].item.id]
        );
        assert_eq!(
            app.canvas.selected_annotation_ids(),
            vec![pasted_annotations[0].id]
        );
        let pasted_stack_order = vec![
            CanvasStackItem::Annotation(pasted_annotations[0].id),
            CanvasStackItem::Placement(pasted_items[0].item.id),
        ];
        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            pasted_stack_order
        );
        library.set_active_board(first.id).unwrap();
        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            source_stack_order
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn paste_preserves_mixed_selection_stack_order_and_places_group_at_front() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-mixed-paste-stack-order");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let back_item = library
            .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let source_item = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(10.0, 20.0), 1.0, 0.0, 0)
            .unwrap();
        let source_annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Line,
                egui::pos2(40.0, 60.0),
                egui::pos2(80.0, 90.0),
                "",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Gold,
                false,
            )
            .unwrap();
        let front_item = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(100.0, 100.0), 1.0, 0.0, 0)
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        for record in [&back_item, &source_item, &front_item] {
            app.canvas.add_loaded_item(
                &ctx,
                LoadedCanvasItem {
                    target_board_id: 1,
                    queue_id: None,
                    record: record.clone(),
                    preview_rgba: vec![255, 0, 0, 255],
                    preview_width: 1,
                    preview_height: 1,
                    animated_preview: None,
                    preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                    loaded_tier: crate::data::CanvasImageCacheTier::Working,
                    existing_load_diagnostics: None,
                    origin: crate::jobs::ImportOrigin::Existing,
                    trace: None,
                },
            );
        }
        app.canvas.add_annotation(source_annotation.clone());
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Placement(back_item.item.id),
            CanvasStackItem::Annotation(source_annotation.id),
            CanvasStackItem::Placement(source_item.item.id),
            CanvasStackItem::Placement(front_item.item.id),
        ]);
        app.canvas
            .select_content(&[source_item.item.id], &[source_annotation.id]);

        let payload = app.copy_selected_board_payload().unwrap();
        app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
            .unwrap();

        let pasted_item_id = app.canvas.selected_item_ids()[0];
        let pasted_annotation_id = app.canvas.selected_annotation_ids()[0];
        assert_eq!(
            app.canvas.canvas_stack_order(),
            vec![
                CanvasStackItem::Placement(back_item.item.id),
                CanvasStackItem::Annotation(source_annotation.id),
                CanvasStackItem::Placement(source_item.item.id),
                CanvasStackItem::Placement(front_item.item.id),
                CanvasStackItem::Annotation(pasted_annotation_id),
                CanvasStackItem::Placement(pasted_item_id),
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn paste_preserves_image_only_stack_order_and_places_group_at_front() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-image-paste-stack-order");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let back_item = library
            .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let lower_selected = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(10.0, 20.0), 1.0, 0.0, 0)
            .unwrap();
        let upper_selected = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(30.0, 40.0), 1.0, 0.0, 0)
            .unwrap();
        let front_item = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(100.0, 100.0), 1.0, 0.0, 0)
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        for record in [&back_item, &lower_selected, &upper_selected, &front_item] {
            add_loaded_record(&mut app, &ctx, record.clone());
        }
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Placement(back_item.item.id),
            CanvasStackItem::Placement(upper_selected.item.id),
            CanvasStackItem::Placement(lower_selected.item.id),
            CanvasStackItem::Placement(front_item.item.id),
        ]);
        app.canvas
            .select_items(&[lower_selected.item.id, upper_selected.item.id]);

        let payload = app.copy_selected_board_payload().unwrap();
        app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
            .unwrap();

        let selected = app.canvas.selected_item_ids();
        let pasted_lower_id = selected[0];
        let pasted_upper_id = selected[1];
        let expected = vec![
            CanvasStackItem::Placement(back_item.item.id),
            CanvasStackItem::Placement(upper_selected.item.id),
            CanvasStackItem::Placement(lower_selected.item.id),
            CanvasStackItem::Placement(front_item.item.id),
            CanvasStackItem::Placement(pasted_upper_id),
            CanvasStackItem::Placement(pasted_lower_id),
        ];
        assert_eq!(app.canvas.canvas_stack_order(), expected);
        assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn paste_preserves_annotation_only_stack_order_and_places_group_at_front() {
        let root = temp_library_root("reference-annotation-paste-stack-order");
        let library = Library::initialize(root.clone()).unwrap();
        let back_annotation = test_canvas_annotation(1, egui::pos2(0.0, 0.0));
        let lower_selected = test_canvas_annotation(2, egui::pos2(10.0, 20.0));
        let upper_selected = test_canvas_annotation(3, egui::pos2(30.0, 40.0));
        let front_annotation = test_canvas_annotation(4, egui::pos2(100.0, 100.0));
        for annotation in [
            &back_annotation,
            &lower_selected,
            &upper_selected,
            &front_annotation,
        ] {
            library
                .create_canvas_annotation(
                    annotation.kind,
                    egui::pos2(annotation.start_x, annotation.start_y),
                    egui::pos2(annotation.end_x, annotation.end_y),
                    &annotation.text,
                    annotation.text_size,
                    annotation.stroke_width,
                    annotation.color,
                    annotation.background_enabled,
                )
                .unwrap();
        }
        let annotations = library.load_canvas_annotations().unwrap();
        let back_id = annotations[0].id;
        let lower_id = annotations[1].id;
        let upper_id = annotations[2].id;
        let front_id = annotations[3].id;
        let mut app = test_app(Some(library.clone()));
        app.canvas.add_annotations(annotations);
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Annotation(back_id),
            CanvasStackItem::Annotation(upper_id),
            CanvasStackItem::Annotation(lower_id),
            CanvasStackItem::Annotation(front_id),
        ]);
        app.canvas.select_content(&[], &[lower_id, upper_id]);

        let payload = app.copy_selected_board_payload().unwrap();
        app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
            .unwrap();

        let selected = app.canvas.selected_annotation_ids();
        let pasted_lower_id = selected[0];
        let pasted_upper_id = selected[1];
        let expected = vec![
            CanvasStackItem::Annotation(back_id),
            CanvasStackItem::Annotation(upper_id),
            CanvasStackItem::Annotation(lower_id),
            CanvasStackItem::Annotation(front_id),
            CanvasStackItem::Annotation(pasted_upper_id),
            CanvasStackItem::Annotation(pasted_lower_id),
        ];
        assert_eq!(app.canvas.canvas_stack_order(), expected);
        assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_offsets_mixed_selection_on_same_board_and_selects_new_content() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-mixed-duplicate");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let source_item = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let source_annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Line,
                egui::pos2(40.0, 60.0),
                egui::pos2(80.0, 90.0),
                "",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Gold,
                false,
            )
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.settings.import_gap = 24.0;
        app.canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record: source_item.clone(),
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        app.canvas.add_annotation(source_annotation.clone());
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Annotation(source_annotation.id),
            CanvasStackItem::Placement(source_item.item.id),
        ]);
        app.canvas
            .select_content(&[source_item.item.id], &[source_annotation.id]);

        app.duplicate_selection_with_gap();

        let items = library.load_board_items().unwrap();
        let annotations = library.load_canvas_annotations().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(annotations.len(), 2);
        let duplicated_item = items
            .iter()
            .find(|item| item.item.id != source_item.item.id)
            .unwrap();
        let duplicated_annotation = annotations
            .iter()
            .find(|annotation| annotation.id != source_annotation.id)
            .unwrap();
        let item_offset = egui::vec2(
            duplicated_item.item.x - source_item.item.x,
            duplicated_item.item.y - source_item.item.y,
        );
        let annotation_offset = egui::vec2(
            duplicated_annotation.start_x - source_annotation.start_x,
            duplicated_annotation.start_y - source_annotation.start_y,
        );
        assert_eq!(annotation_offset, item_offset);
        assert_ne!(item_offset, egui::Vec2::ZERO);
        assert_eq!(
            app.canvas.selected_item_ids(),
            vec![duplicated_item.item.id]
        );
        assert_eq!(
            app.canvas.selected_annotation_ids(),
            vec![duplicated_annotation.id]
        );
        let expected = vec![
            CanvasStackItem::Annotation(source_annotation.id),
            CanvasStackItem::Placement(source_item.item.id),
            CanvasStackItem::Annotation(duplicated_annotation.id),
            CanvasStackItem::Placement(duplicated_item.item.id),
        ];
        assert_eq!(app.canvas.canvas_stack_order(), expected);
        assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_preserves_image_only_stack_order_and_places_group_at_front() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-image-duplicate-stack-order");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let back_item = library
            .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let lower_selected = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(10.0, 20.0), 1.0, 0.0, 0)
            .unwrap();
        let upper_selected = library
            .create_board_item_from_asset(back_item.asset.id, egui::pos2(30.0, 40.0), 1.0, 0.0, 0)
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.settings.import_gap = 24.0;
        for record in [&back_item, &lower_selected, &upper_selected] {
            add_loaded_record(&mut app, &ctx, record.clone());
        }
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Placement(back_item.item.id),
            CanvasStackItem::Placement(upper_selected.item.id),
            CanvasStackItem::Placement(lower_selected.item.id),
        ]);
        app.canvas
            .select_items(&[lower_selected.item.id, upper_selected.item.id]);

        app.duplicate_selection_with_gap();

        let selected = app.canvas.selected_item_ids();
        let duplicated_lower_id = selected[0];
        let duplicated_upper_id = selected[1];
        let expected = vec![
            CanvasStackItem::Placement(back_item.item.id),
            CanvasStackItem::Placement(upper_selected.item.id),
            CanvasStackItem::Placement(lower_selected.item.id),
            CanvasStackItem::Placement(duplicated_upper_id),
            CanvasStackItem::Placement(duplicated_lower_id),
        ];
        assert_eq!(app.canvas.canvas_stack_order(), expected);
        assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_preserves_annotation_only_stack_order_and_places_group_at_front() {
        let root = temp_library_root("reference-annotation-duplicate-stack-order");
        let library = Library::initialize(root.clone()).unwrap();
        let back_annotation = test_canvas_annotation(1, egui::pos2(0.0, 0.0));
        let lower_selected = test_canvas_annotation(2, egui::pos2(10.0, 20.0));
        let upper_selected = test_canvas_annotation(3, egui::pos2(30.0, 40.0));
        for annotation in [&back_annotation, &lower_selected, &upper_selected] {
            library
                .create_canvas_annotation(
                    annotation.kind,
                    egui::pos2(annotation.start_x, annotation.start_y),
                    egui::pos2(annotation.end_x, annotation.end_y),
                    &annotation.text,
                    annotation.text_size,
                    annotation.stroke_width,
                    annotation.color,
                    annotation.background_enabled,
                )
                .unwrap();
        }
        let annotations = library.load_canvas_annotations().unwrap();
        let back_id = annotations[0].id;
        let lower_id = annotations[1].id;
        let upper_id = annotations[2].id;
        let mut app = test_app(Some(library.clone()));
        app.settings.import_gap = 24.0;
        app.canvas.add_annotations(annotations);
        app.canvas.set_canvas_stack_order(vec![
            CanvasStackItem::Annotation(back_id),
            CanvasStackItem::Annotation(upper_id),
            CanvasStackItem::Annotation(lower_id),
        ]);
        app.canvas.select_content(&[], &[lower_id, upper_id]);

        app.duplicate_selection_with_gap();

        let selected = app.canvas.selected_annotation_ids();
        let duplicated_lower_id = selected[0];
        let duplicated_upper_id = selected[1];
        let expected = vec![
            CanvasStackItem::Annotation(back_id),
            CanvasStackItem::Annotation(upper_id),
            CanvasStackItem::Annotation(lower_id),
            CanvasStackItem::Annotation(duplicated_upper_id),
            CanvasStackItem::Annotation(duplicated_lower_id),
        ];
        assert_eq!(app.canvas.canvas_stack_order(), expected);
        assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_a_selects_all_active_board_images_and_annotations() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        for id in [1_i64, 2] {
            app.canvas.add_loaded_item(&ctx, test_loaded_item(id, 1));
        }
        app.canvas.add_annotation(crate::data::CanvasAnnotation {
            id: 10,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 0.0,
            start_y: 0.0,
            end_x: 100.0,
            end_y: 40.0,
            text: "one".to_owned(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Default,
            background_enabled: false,
            archived: false,
        });
        app.canvas.add_annotation(crate::data::CanvasAnnotation {
            id: 11,
            kind: CanvasAnnotationKind::Line,
            start_x: 20.0,
            start_y: 20.0,
            end_x: 80.0,
            end_y: 80.0,
            text: String::new(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Default,
            background_enabled: false,
            archived: false,
        });
        app.canvas.select_content(&[], &[]);

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: Some(egui::Key::A),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| {
                app.handle_selection_shortcuts(
                    ui.ctx(),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
                )
            },
        );

        assert_eq!(app.canvas.selected_item_ids(), vec![1, 2]);
        assert_eq!(app.canvas.selected_annotation_ids(), vec![10, 11]);
    }

    #[test]
    fn copy_selection_emits_internal_json_text_without_clipboard_image_data() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.canvas.add_loaded_item(&ctx, test_loaded_item(1, 1));
        app.canvas.add_annotation(crate::data::CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 10.0,
            start_y: 10.0,
            end_x: 70.0,
            end_y: 40.0,
            text: "caption".to_owned(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Default,
            background_enabled: false,
            archived: false,
        });
        app.canvas.select_content(&[1], &[20]);

        let mut commands = Vec::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ui| {
                app.handle_clipboard_copy(ui.ctx());
                commands = ui.ctx().output(|output| output.commands.clone());
            },
        );

        assert_eq!(commands.len(), 1);
        let egui::OutputCommand::CopyText(payload) = &commands[0] else {
            panic!("copy should emit text payload only");
        };
        let payload =
            serde_json::from_str::<crate::active_board_commands::ClipboardBoardSelection>(payload)
                .unwrap();
        assert_eq!(
            payload.kind,
            crate::active_board_commands::COPY_PAYLOAD_KIND
        );
        assert_eq!(payload.items.len(), 1);
        assert_eq!(payload.annotations.len(), 1);
        assert_eq!(
            payload
                .stack_order
                .iter()
                .map(|entry| (entry.kind.as_str(), entry.index))
                .collect::<Vec<_>>(),
            vec![("placement", 0), ("annotation", 0)]
        );
    }

    #[test]
    fn context_menu_items_reflect_mixed_selection_capabilities() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.canvas.add_loaded_item(&ctx, test_loaded_item(1, 1));
        app.canvas.add_annotation(crate::data::CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 10.0,
            start_y: 10.0,
            end_x: 70.0,
            end_y: 40.0,
            text: "caption".to_owned(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Default,
            background_enabled: false,
            archived: false,
        });
        app.canvas.select_content(&[1], &[20]);

        let items = app.context_menu_items();

        assert_eq!(
            items.iter().map(|item| item.label).collect::<Vec<_>>(),
            vec![
                "Copy",
                "Paste",
                "Duplicate",
                "Delete (from board)",
                "Move to Front",
                "Move to Back",
                "Search using image",
                "Remove from Library..."
            ]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| (item.action, item.shortcut))
                .collect::<Vec<_>>(),
            vec![
                (super::ContextMenuAction::Copy, Some("⌘ C")),
                (super::ContextMenuAction::Paste, Some("⌘ V")),
                (super::ContextMenuAction::Duplicate, Some("⌘ D")),
                (super::ContextMenuAction::DeleteFromBoard, Some("del")),
                (super::ContextMenuAction::MoveToFront, Some("⌘ .")),
                (super::ContextMenuAction::MoveToBack, Some("⌘ ,")),
                (super::ContextMenuAction::SearchUsingImage, Some("⌘ F")),
                (super::ContextMenuAction::RemoveFromLibrary, None),
            ]
        );
        assert!(item_enabled(&items, super::ContextMenuAction::Copy));
        assert!(item_enabled(&items, super::ContextMenuAction::Paste));
        assert!(item_enabled(&items, super::ContextMenuAction::Duplicate));
        assert!(item_enabled(
            &items,
            super::ContextMenuAction::DeleteFromBoard
        ));
        assert!(item_enabled(&items, super::ContextMenuAction::MoveToFront));
        assert!(item_enabled(&items, super::ContextMenuAction::MoveToBack));
        assert!(!item_enabled(
            &items,
            super::ContextMenuAction::SearchUsingImage
        ));
        assert!(item_enabled(
            &items,
            super::ContextMenuAction::RemoveFromLibrary
        ));
    }

    #[test]
    fn context_menu_primary_press_inside_menu_keeps_menu_open_for_click_release() {
        let menu_rect =
            egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));

        assert!(!super::context_menu_should_close(
            false,
            false,
            menu_rect,
            Some(menu_rect.center()),
            true,
            false,
            false,
        ));
    }

    #[test]
    fn context_menu_primary_press_outside_menu_closes_menu() {
        let menu_rect =
            egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));

        assert!(super::context_menu_should_close(
            false,
            false,
            menu_rect,
            Some(egui::pos2(menu_rect.right() + 8.0, menu_rect.center().y)),
            true,
            false,
            false,
        ));
    }

    #[test]
    fn open_context_menu_blocks_canvas_input_behind_menu() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);

        assert!(!app.blocks_canvas_input(&ctx));

        app.context_menu_pos = Some(egui::pos2(520.0, 260.0));

        assert!(app.blocks_canvas_input(&ctx));
    }

    #[test]
    fn stack_order_context_actions_are_disabled_without_canvas_item_selection() {
        let app = test_app(None);

        let items = app.context_menu_items();

        assert!(!item_enabled(&items, super::ContextMenuAction::MoveToFront));
        assert!(!item_enabled(&items, super::ContextMenuAction::MoveToBack));
    }

    #[test]
    fn stack_order_context_actions_are_enabled_for_image_and_annotation_selections() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.canvas.add_loaded_item(&ctx, test_loaded_item(1, 1));
        app.canvas.add_annotation(crate::data::CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 10.0,
            start_y: 10.0,
            end_x: 70.0,
            end_y: 40.0,
            text: "caption".to_owned(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Default,
            background_enabled: false,
            archived: false,
        });

        app.canvas.select_content(&[1], &[]);
        let image_only = app.context_menu_items();
        assert!(item_enabled(
            &image_only,
            super::ContextMenuAction::MoveToFront
        ));
        assert!(item_enabled(
            &image_only,
            super::ContextMenuAction::MoveToBack
        ));

        app.canvas.select_content(&[], &[20]);
        let annotation_only = app.context_menu_items();
        assert!(item_enabled(
            &annotation_only,
            super::ContextMenuAction::MoveToFront
        ));
        assert!(item_enabled(
            &annotation_only,
            super::ContextMenuAction::MoveToBack
        ));
    }

    #[test]
    fn image_search_context_action_requires_exactly_one_image() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.canvas.add_loaded_item(&ctx, test_loaded_item(1, 1));
        app.canvas.add_loaded_item(&ctx, test_loaded_item(2, 1));

        app.canvas.select_items(&[1]);
        let single = app.context_menu_items();
        assert!(item_enabled(
            &single,
            super::ContextMenuAction::SearchUsingImage
        ));

        app.canvas.select_items(&[1, 2]);
        let multiple = app.context_menu_items();
        assert!(!item_enabled(
            &multiple,
            super::ContextMenuAction::SearchUsingImage
        ));
    }

    #[test]
    fn remove_from_library_context_action_waits_for_confirmation() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-context-remove-confirm");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let source_item = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record: source_item.clone(),
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        app.canvas.select_items(&[source_item.item.id]);

        app.handle_context_menu_action(
            super::ContextMenuAction::RemoveFromLibrary,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        assert!(app.pending_library_remove.is_some());
        assert_eq!(library.load_board_items().unwrap().len(), 1);

        app.confirm_pending_library_remove();

        assert!(app.pending_library_remove.is_none());
        assert!(library.load_board_items().unwrap().is_empty());
        assert!(source_item.asset.original_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_to_front_context_action_persists_canvas_stack_order() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-context-move-front");
        let library = Library::initialize(root.clone()).unwrap();
        let first_path = library
            .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let second_path = library
            .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let first = library
            .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let second = library
            .persist_imported_image(
                &second_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "hello",
                CanvasTextSize::default(),
                CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.canvas
            .add_loaded_item(&ctx, test_loaded_item(first.item.id, 1));
        app.canvas
            .add_loaded_item(&ctx, test_loaded_item(second.item.id, 1));
        app.canvas.add_annotation(annotation.clone());
        app.canvas.set_canvas_stack_order(vec![
            crate::data::CanvasStackItem::Placement(first.item.id),
            crate::data::CanvasStackItem::Annotation(annotation.id),
            crate::data::CanvasStackItem::Placement(second.item.id),
        ]);
        app.canvas
            .select_content(&[first.item.id], &[annotation.id]);

        app.handle_context_menu_action(
            super::ContextMenuAction::MoveToFront,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            vec![
                crate::data::CanvasStackItem::Placement(second.item.id),
                crate::data::CanvasStackItem::Placement(first.item.id),
                crate::data::CanvasStackItem::Annotation(annotation.id),
            ]
        );

        app.handle_context_menu_action(
            super::ContextMenuAction::MoveToBack,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        assert_eq!(
            library.load_canvas_stack_order().unwrap(),
            vec![
                crate::data::CanvasStackItem::Placement(first.item.id),
                crate::data::CanvasStackItem::Annotation(annotation.id),
                crate::data::CanvasStackItem::Placement(second.item.id),
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_from_board_archives_image_placements_but_hard_deletes_annotations() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-delete-from-board");
        let library = Library::initialize(root.clone()).unwrap();
        let image_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let source_item = library
            .persist_imported_image(
                &image_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Line,
                egui::pos2(0.0, 0.0),
                egui::pos2(20.0, 20.0),
                "",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Default,
                false,
            )
            .unwrap();
        let mut app = test_app(Some(library.clone()));
        app.canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record: source_item.clone(),
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        app.canvas.add_annotation(annotation.clone());
        app.canvas
            .select_content(&[source_item.item.id], &[annotation.id]);

        app.handle_context_menu_action(
            super::ContextMenuAction::DeleteFromBoard,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        assert!(library.load_board_items().unwrap().is_empty());
        assert!(
            library
                .asset_original_paths_for_assets(&[source_item.asset.id])
                .unwrap()
                .contains_key(&source_item.asset.id)
        );
        let connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
        let annotation_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM canvas_annotations WHERE id = ?1",
                [annotation.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(annotation_count, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_using_image_context_action_opens_image_pivot_search() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.settings.semantic_search_enabled = true;
        app.canvas.add_loaded_item(&ctx, test_loaded_item(42, 1));
        app.canvas.select_items(&[42]);

        app.handle_context_menu_action(
            super::ContextMenuAction::SearchUsingImage,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        assert!(app.search.overlay.open);
        assert!(app.search.overlay.suppress_next_outside_click);
        let pivot = app.search.overlay.image_pivot.unwrap();
        assert_eq!(pivot.asset_id, 42);
        assert_eq!(pivot.item_id, 42);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn search_using_image_context_action_survives_activation_click_outside_overlay() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.settings.semantic_search_enabled = true;
        app.canvas.add_loaded_item(&ctx, test_loaded_item(42, 1));
        app.canvas.select_items(&[42]);

        app.handle_context_menu_action(
            super::ContextMenuAction::SearchUsingImage,
            &ctx,
            egui::Rect::EVERYTHING,
        );

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(520.0, 320.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(520.0, 320.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |ui| {
                app.show_search_overlay(
                    ui.ctx(),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
                )
            },
        );

        assert!(app.search.overlay.open);
        assert!(!app.search.overlay.suppress_next_outside_click);
    }

    #[test]
    fn sidebar_create_board_generates_name_switches_and_stays_open() {
        let root = temp_library_root("reference-create-board");
        let library = Library::initialize(root.clone()).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.sidebar_open = true;

        app.create_and_switch_board(true);

        let boards = library.list_boards().unwrap();
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].name, "My Board");
        assert_eq!(boards[1].name, "Board 2");
        assert_eq!(app.active_board_id, boards[1].id);
        assert_eq!(
            library.load_user_session().unwrap().active_board_id,
            boards[1].id
        );
        assert!(app.sidebar_open);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_board_names_fill_smallest_visible_gap() {
        let root = temp_library_root("reference-board-name-gap");
        let library = Library::initialize(root.clone()).unwrap();
        let board_2 = library.create_board("Board 2").unwrap();
        library.create_board("Board 3").unwrap();
        {
            let connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
            connection
                .execute("UPDATE boards SET archived = 1 WHERE id = ?1", [board_2.id])
                .unwrap();
        }
        let mut app = test_app(Some(library.clone()));

        app.create_and_switch_board(false);

        let boards = library.list_boards().unwrap();
        assert!(boards.iter().any(|board| board.name == "Board 2"));
        assert_eq!(app.active_board_id, boards.last().unwrap().id);
        assert!(!app.sidebar_open);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_n_creates_board_without_opening_sidebar() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-cmd-n-board");
        let library = Library::initialize(root.clone()).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.sidebar_open = false;

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::N,
                    physical_key: Some(egui::Key::N),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| app.handle_board_shortcuts(ui.ctx()),
        );

        let boards = library.list_boards().unwrap();
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[1].name, "Board 2");
        assert_eq!(app.active_board_id, boards[1].id);
        assert!(!app.sidebar_open);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_number_switches_visible_board() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-cmd-number-board");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let third = library.create_board("Third").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = second.id;
        library.set_active_board(second.id).unwrap();

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::Num3,
                    physical_key: Some(egui::Key::Num3),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| app.handle_board_shortcuts(ui.ctx()),
        );

        assert_eq!(app.active_board_id, third.id);
        assert_eq!(
            library.load_user_session().unwrap().active_board_id,
            third.id
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn command_zero_switches_tenth_visible_board() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-cmd-zero-board");
        let library = Library::initialize(root.clone()).unwrap();
        let mut tenth = None;
        for index in 2..=10 {
            let board = library.create_board(&format!("Board {index}")).unwrap();
            if index == 10 {
                tenth = Some(board);
            }
        }
        let tenth = tenth.unwrap();
        let mut app = test_app(Some(library.clone()));

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::Num0,
                    physical_key: Some(egui::Key::Num0),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| app.handle_board_shortcuts(ui.ctx()),
        );

        assert_eq!(app.active_board_id, tenth.id);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn board_shortcuts_are_suppressed_while_search_is_open() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-board-shortcut-search");
        let library = Library::initialize(root.clone()).unwrap();
        library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.search.overlay.open = true;

        let _ = ctx.run_ui(
            egui::RawInput {
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
                events: vec![egui::Event::Key {
                    key: egui::Key::Num2,
                    physical_key: Some(egui::Key::Num2),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        command: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            },
            |ui| app.handle_board_shortcuts(ui.ctx()),
        );

        assert_eq!(app.active_board_id, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn switching_boards_saves_previous_viewport_and_loads_target_viewport() {
        let root = temp_library_root("reference-board-switch-viewport");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        library.set_active_board(second.id).unwrap();
        let mut second_session = library.load_user_session().unwrap();
        second_session.viewport.camera_pan_x = 75.0;
        second_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
        library.save_user_session(&second_session).unwrap();
        library.set_active_board(first.id).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = first.id;
        app.canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 22.0,
                camera_pan_y: 33.0,
                camera_zoom: 1.5,
                canvas_background: crate::data::CanvasBackground::Grid,
            });

        app.switch_to_board(second.id);

        let saved_first = {
            library.set_active_board(first.id).unwrap();
            library.load_user_session().unwrap()
        };
        assert_eq!(saved_first.viewport.camera_pan_x, 22.0);
        assert_eq!(
            saved_first.viewport.canvas_background,
            crate::data::CanvasBackground::Grid
        );
        library.set_active_board(second.id).unwrap();
        assert_eq!(app.active_board_id, second.id);
        assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn active_board_load_applies_viewport_without_reloading_global_settings() {
        let root = temp_library_root("reference-active-load-settings-scope");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        library.set_active_board(second.id).unwrap();
        let mut stored_session = library.load_user_session().unwrap();
        stored_session.settings.import_gap = 99.0;
        stored_session.viewport.camera_pan_x = 75.0;
        stored_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
        library.save_user_session(&stored_session).unwrap();

        let mut app = test_app(Some(library.clone()));
        app.settings.import_gap = 24.0;

        app.load_active_board();

        assert_eq!(app.active_board_id, second.id);
        assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);
        assert_eq!(app.canvas.viewport_state().camera_pan_x, 75.0);
        assert_eq!(app.settings.import_gap, 24.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_delete_requires_confirmation_before_archiving_active_board() {
        let root = temp_library_root("reference-confirm-delete-board");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        library.set_active_board(second.id).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = second.id;
        app.sidebar_open = true;

        app.request_board_delete(second.id, second.name.clone());

        assert!(app.pending_board_delete.is_some());
        assert_eq!(library.list_boards().unwrap().len(), 2);

        app.confirm_pending_board_delete();

        assert!(app.pending_board_delete.is_none());
        assert!(app.sidebar_open);
        assert_eq!(app.active_board_id, first.id);
        assert_eq!(
            library
                .list_boards()
                .unwrap()
                .iter()
                .map(|board| board.id)
                .collect::<Vec<_>>(),
            vec![first.id]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_delete_confirmation_keeps_sidebar_open_and_removes_inactive_board() {
        let root = temp_library_root("reference-delete-inactive-board");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = first.id;
        app.sidebar_open = true;

        app.request_board_delete(second.id, second.name.clone());
        app.confirm_pending_board_delete();

        assert!(app.pending_board_delete.is_none());
        assert!(app.sidebar_open);
        assert_eq!(app.active_board_id, first.id);
        assert_eq!(
            library
                .list_boards()
                .unwrap()
                .iter()
                .map(|board| board.id)
                .collect::<Vec<_>>(),
            vec![first.id]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_delete_modal_click_routes_through_egui_layers() {
        let root = temp_library_root("reference-delete-modal-click-routing");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = second.id;
        app.sidebar_open = true;
        app.request_board_delete(second.id, second.name.clone());
        #[expect(deprecated)]
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_state(
                |ctx, app: &mut ReferenceApp| {
                    let screen_rect = test_screen_rect();
                    if app.should_block_sidebar_input() {
                        block_sidebar_input(ctx, sidebar_rect(screen_rect));
                    }
                    app.show_board_delete_confirmation(ctx);
                },
                app,
            );

        harness.get_by_label("Delete board").click();
        harness.run();

        assert!(
            harness.state().pending_board_delete.is_none(),
            "the modal should receive the delete-button click"
        );
        assert!(harness.state().sidebar_open);
        assert_eq!(harness.state().active_board_id, first.id);
        assert_eq!(
            library
                .list_boards()
                .unwrap()
                .iter()
                .map(|board| board.id)
                .collect::<Vec<_>>(),
            vec![first.id]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_sidebar_stays_clickable_after_delete_modal_confirms() {
        let root = temp_library_root("reference-sidebar-click-after-delete");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = second.id;
        app.sidebar_open = true;
        app.sidebar_tab = SidebarTab::Boards;
        app.request_board_delete(second.id, second.name.clone());
        #[expect(deprecated)]
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_state(
                |ctx, app: &mut ReferenceApp| {
                    render_sidebar_for_test(ctx, app);
                    app.show_board_delete_confirmation(ctx);
                },
                app,
            );

        harness.get_by_label("Delete board").click();
        harness.run();

        click_test_pos(&harness, egui::pos2(134.0, 171.0));
        harness.run();

        let boards = library.list_boards().unwrap();
        assert_eq!(
            boards
                .iter()
                .map(|board| board.name.as_str())
                .collect::<Vec<_>>(),
            vec!["My Board", "Board 2"]
        );
        assert_eq!(harness.state().active_board_id, boards[1].id);
        assert!(harness.state().sidebar_open);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_sidebar_name_click_switches_board_immediately() {
        let root = temp_library_root("reference-sidebar-name-click-switch");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = first.id;
        app.sidebar_open = true;
        app.sidebar_tab = SidebarTab::Boards;
        #[expect(deprecated)]
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_state(
                |ctx, app: &mut ReferenceApp| {
                    render_sidebar_for_test(ctx, app);
                },
                app,
            );

        click_test_pos(&harness, egui::pos2(64.0, 160.0));
        harness.run();

        assert_eq!(harness.state().active_board_id, second.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rapid_sidebar_board_name_clicks_keep_last_requested_board_active() {
        let root = temp_library_root("reference-rapid-sidebar-board-clicks");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let _third = library.create_board("Third").unwrap();
        let mut app = test_app(Some(library.clone()));
        app.active_board_id = first.id;
        app.sidebar_open = true;
        app.sidebar_tab = SidebarTab::Boards;
        #[expect(deprecated)]
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .build_state(
                |ctx, app: &mut ReferenceApp| {
                    render_sidebar_for_test(ctx, app);
                },
                app,
            );

        for pos in [
            egui::pos2(64.0, 160.0),
            egui::pos2(64.0, 212.0),
            egui::pos2(64.0, 108.0),
            egui::pos2(64.0, 160.0),
        ] {
            click_test_pos(&harness, pos);
            harness.run();
        }

        assert_eq!(harness.state().active_board_id, second.id);
        assert_eq!(
            library.load_user_session().unwrap().active_board_id,
            second.id
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rapid_board_switches_ignore_stale_import_completions_and_last_switch_wins() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-rapid-switch-stale-imports");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let third = library.create_board("Third").unwrap();
        let mut app = test_app(Some(library));
        app.active_board_id = first.id;

        app.switch_to_board(second.id);
        app.integrate_loaded_import(&ctx, test_loaded_item(41, first.id));
        app.switch_to_board(third.id);
        app.integrate_loaded_import(&ctx, test_loaded_item(42, second.id));
        app.switch_to_board(first.id);
        app.integrate_loaded_import(&ctx, test_loaded_item(43, third.id));

        assert_eq!(app.active_board_id, first.id);
        assert!(app.canvas.record_for_item_id(41).is_none());
        assert!(app.canvas.record_for_item_id(42).is_none());
        assert!(app.canvas.record_for_item_id(43).is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(feature = "semantic-search")]
    #[test]
    fn switching_boards_clears_selection_search_and_transient_undo() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-board-switch-cleanup");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library.clone()));
        let asset = ImageAsset {
            id: 1,
            original_path: PathBuf::from("fixture.png"),
            filename: "fixture.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 1,
            height: 1,
            file_size: 4,
            imported_at: 0,
        };
        let record = BoardItemWithAsset {
            item: BoardItem {
                id: 1,
                asset_id: 1,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset,
        };
        app.canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record: record.clone(),
                preview_rgba: vec![255, 255, 255, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        app.canvas.select_items(&[1]);
        app.search.overlay.open = true;
        app.canvas.set_search_matches(vec![1], Some(1));
        app.crop_flow.push_test_undo_entry(CropUndoEntry {
            previous_record: record,
            previous_rgba: vec![255, 255, 255, 255],
            previous_preview_width: 1,
            previous_preview_height: 1,
            removed_asset_id: 2,
        });

        app.switch_to_board(second.id);

        assert!(!app.canvas.has_selection());
        assert!(!app.search.is_open());
        assert!(app.crop_flow.undo_stack_is_empty());

        let _ = fs::remove_dir_all(root);
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
    fn tab_switches_search_scope_from_any_search_overlay_focus_state() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.search.overlay.open = true;
        app.search.overlay.focus_requested = true;
        app.search.overlay.scope = SearchScope::ThisBoard;

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: Some(egui::Key::Tab),
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

        assert_eq!(app.search.overlay.scope, SearchScope::Library);
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
                    target_board_id: 1,
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

        app.queue_import_placeholder_for_board(
            app.active_board_id,
            7,
            "first".to_owned(),
            egui::pos2(10.0, 20.0),
            300,
            200,
        );
        app.queue_import_placeholder_for_board(
            app.active_board_id,
            7,
            "second".to_owned(),
            egui::pos2(30.0, 40.0),
            120,
            90,
        );

        assert_eq!(app.canvas.import_placeholder_count(), 1);
        app.canvas.remove_import_placeholder(7);
        assert_eq!(app.canvas.import_placeholder_count(), 0);
    }

    #[test]
    fn reserve_import_world_accounts_for_pending_placeholders() {
        let mut app = test_app(None);
        app.queue_import_placeholder_for_board(
            app.active_board_id,
            1,
            "first".to_owned(),
            egui::pos2(0.0, 0.0),
            100,
            100,
        );

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
    fn import_reservation_captures_board_queue_and_optional_placeholder() {
        let root = temp_library_root("reference-import-reservation");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library));
        app.active_board_id = second.id;
        let mut layout = Vec::new();

        let known = app.reserve_import(
            &mut layout,
            "known".to_owned(),
            Some((100, 80)),
            egui::Pos2::ZERO,
            test_viewport_world(),
        );
        let unknown = app.reserve_import(
            &mut layout,
            "unknown".to_owned(),
            None,
            egui::pos2(200.0, 0.0),
            test_viewport_world(),
        );

        assert_eq!(known.target_board_id, second.id);
        assert_eq!(known.queue_id, 1);
        assert_eq!(known.reserved_world, Some(egui::Pos2::ZERO));
        assert_eq!(unknown.target_board_id, second.id);
        assert_eq!(unknown.queue_id, 2);
        assert_eq!(unknown.reserved_world, None);
        assert_eq!(app.canvas.import_placeholder_count(), 1);
        assert_eq!(layout.len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_import_for_inactive_board_does_not_enter_active_canvas() {
        let ctx = egui::Context::default();
        let root = temp_library_root("reference-inactive-import-completion");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library));
        app.active_board_id = 1;
        let loaded = test_loaded_item(41, second.id);

        app.integrate_loaded_import(&ctx, loaded);

        assert!(app.canvas.record_for_item_id(41).is_none());
        assert_eq!(app.status, "Imported image");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completed_import_for_active_board_enters_active_canvas() {
        let ctx = egui::Context::default();
        let mut app = test_app(None);
        app.active_board_id = 1;
        let loaded = test_loaded_item(42, 1);

        app.integrate_loaded_import(&ctx, loaded);

        assert!(app.canvas.record_for_item_id(42).is_some());
        assert_eq!(app.status, "Imported image");
    }

    #[test]
    fn queued_import_placeholders_are_scoped_to_their_board_across_switches() {
        let root = temp_library_root("reference-placeholder-board-scope");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let mut app = test_app(Some(library));
        app.active_board_id = 1;
        app.queue_import_placeholder_for_board(
            app.active_board_id,
            9,
            "first".to_owned(),
            egui::pos2(10.0, 20.0),
            100,
            100,
        );

        app.switch_to_board(second.id);
        assert_eq!(app.canvas.import_placeholder_count(), 0);

        app.switch_to_board(1);
        assert_eq!(app.canvas.import_placeholder_count(), 1);

        let _ = fs::remove_dir_all(root);
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

    #[test]
    fn import_requests_preserve_import_area_cap_when_auto_scaling_setting_is_off() {
        let mut app = test_app(None);
        app.settings.auto_image_scaling = false;

        let config = app.import_request_config();

        assert!(config.auto_image_scaling);
    }

    fn test_app(library: Option<Library>) -> ReferenceApp {
        ReferenceApp {
            canvas: crate::canvas::CanvasState::default(),
            screenshot_capture: crate::screenshot::ScreenshotCapture::new(None),
            window_capture: None,
            wgpu_render_state: None,
            library,
            import_worker: None,
            active_board_id: 1,
            crop_flow: super::CropFlow::default(),
            max_texture_side: 8192,
            settings: crate::data::WorkspaceSettings::default(),
            #[cfg(feature = "semantic-search")]
            search: super::SearchController::default(),
            frame_perf: super::FramePerfState::default(),
            import_intake: super::ImportIntake::new(),
            residency: super::ImageResidencyCoordinator::default(),
            sidebar_open: false,
            sidebar_tab: super::SidebarTab::default(),
            pending_board_delete: None,
            pending_library_remove: None,
            context_menu_pos: None,
            status: String::new(),
            perf_log: None,
            hover_preview_dimensions: std::collections::HashMap::new(),
            document_persistence: super::DocumentPersistence::default(),
            #[cfg(debug_assertions)]
            verification_mode: None,
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            search_latency_probe: None,
            #[cfg(debug_assertions)]
            verification_search_query: None,
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            verification_search_scope: crate::semantic_search::SearchScope::ThisBoard,
            #[cfg(debug_assertions)]
            sidebar_hot_preview: false,
        }
    }

    fn test_screen_rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))
    }

    fn render_sidebar_for_test(ctx: &egui::Context, app: &mut ReferenceApp) {
        let screen_rect = test_screen_rect();
        if app.should_block_sidebar_input() {
            block_sidebar_input(ctx, sidebar_rect(screen_rect));
        }
        let board_summaries = app.board_summaries();
        let sidebar_result = show_sidebar(
            ctx,
            screen_rect,
            app.sidebar_open,
            &mut app.sidebar_tab,
            &mut app.settings,
            &mut app.canvas,
            &board_summaries,
            app.active_board_id,
            None,
        );
        app.handle_board_action(sidebar_result.board_action);
    }

    fn click_test_pos<State>(harness: &Harness<'_, State>, pos: egui::Pos2) {
        harness.hover_at(pos);
        harness.drag_at(pos);
        harness.drop_at(pos);
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

    fn item_enabled(items: &[super::ContextMenuItem], action: super::ContextMenuAction) -> bool {
        items
            .iter()
            .find(|item| item.action == action)
            .is_some_and(|item| item.enabled)
    }

    fn test_loaded_item(item_id: i64, target_board_id: i64) -> LoadedCanvasItem {
        let asset = ImageAsset {
            id: item_id,
            original_path: PathBuf::from(format!("fixture-{item_id}.png")),
            filename: format!("fixture-{item_id}.png"),
            mime_type: "image/png".to_owned(),
            width: 1,
            height: 1,
            file_size: 4,
            imported_at: 0,
        };
        let record = BoardItemWithAsset {
            item: BoardItem {
                id: item_id,
                asset_id: item_id,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset,
        };
        LoadedCanvasItem {
            target_board_id,
            queue_id: Some(item_id as u64),
            record,
            preview_rgba: vec![255, 255, 255, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_tier: crate::data::CanvasImageCacheTier::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Clipboard,
            trace: None,
        }
    }

    fn add_loaded_record(app: &mut ReferenceApp, ctx: &egui::Context, record: BoardItemWithAsset) {
        app.canvas.add_loaded_item(
            ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record,
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
    }

    fn test_canvas_annotation(id: i64, start: egui::Pos2) -> crate::data::CanvasAnnotation {
        crate::data::CanvasAnnotation {
            id,
            kind: CanvasAnnotationKind::Line,
            start_x: start.x,
            start_y: start.y,
            end_x: start.x + 40.0,
            end_y: start.y + 30.0,
            text: String::new(),
            text_size: CanvasTextSize::M,
            stroke_width: CanvasStrokeWidth::M,
            color: crate::data::CanvasAnnotationColor::Gold,
            background_enabled: false,
            archived: false,
        }
    }
}
