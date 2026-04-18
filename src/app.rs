use eframe::egui;
use serde::{Deserialize, Serialize};

const SELECTION_TOOLBAR_WIDTH: f32 = 44.0;
const MULTI_SELECTION_TOOLBAR_WIDTH: f32 = 252.0;
const SELECTION_TOOLBAR_HEIGHT: f32 = 36.0;
const SELECTION_TOOLBAR_GAP: f32 = 12.0;
const SHORTCUT_DRAWER_MIN_HEIGHT: f32 = 220.0;
const SHORTCUT_DRAWER_MAX_HEIGHT: f32 = 360.0;
const SHORTCUT_DRAWER_MARGIN: f32 = 12.0;
const SHORTCUT_DRAWER_BUTTON_SIZE: f32 = 34.0;
const SHORTCUT_SECTION_CARD_RADIUS: u8 = 12;
const ARCHIVE_ICON: egui::ImageSource<'static> =
    egui::include_image!("../assets/icons/iconoir-archive.svg");
const COPY_PAYLOAD_KIND: &str = "reference-board-items";
const MAX_CROP_UNDO_ENTRIES: usize = 32;

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
    ToggleDrawer,
    Arrange(ArrangementAction),
}

#[derive(Clone, Copy)]
struct ShortcutSection {
    title: &'static str,
    rows: &'static [ShortcutRow],
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
        action: "Crop image",
        windows: "C + Drag",
        mac: "C + Drag",
        binding: None,
        shortcut_action: None,
    },
    SupportedShortcut {
        action: "Delete image",
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
        action: "Toggle drawer",
        windows: "Ctrl+B",
        mac: "⌘B",
        binding: Some(ShortcutBinding {
            key: egui::Key::B,
            command: true,
            shift: false,
        }),
        shortcut_action: Some(SupportedShortcutAction::ToggleDrawer),
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
];

const SHORTCUT_SECTIONS: &[ShortcutSection] = &[
    ShortcutSection {
        title: "Actions",
        rows: &[
            ShortcutRow {
                action: "Align bottom",
                windows: "Ctrl+Down",
                mac: "⌘↓",
            },
            ShortcutRow {
                action: "Align left",
                windows: "Ctrl+Left",
                mac: "⌘←",
            },
            ShortcutRow {
                action: "Align right",
                windows: "Ctrl+Right",
                mac: "⌘→",
            },
            ShortcutRow {
                action: "Align top",
                windows: "Ctrl+Up",
                mac: "⌘↑",
            },
            ShortcutRow {
                action: "Arrange by name",
                windows: "Ctrl+Alt+N",
                mac: "⌥⌘N",
            },
            ShortcutRow {
                action: "Duplicate",
                windows: "Ctrl+D",
                mac: "⌘D",
            },
            ShortcutRow {
                action: "Toggle drawer",
                windows: "Ctrl+B",
                mac: "⌘B",
            },
            ShortcutRow {
                action: "Save",
                windows: "Ctrl+S",
                mac: "⌘S",
            },
            ShortcutRow {
                action: "Undo crop",
                windows: "Ctrl+Z",
                mac: "⌘Z",
            },
        ],
    },
    ShortcutSection {
        title: "Controls",
        rows: &[
            ShortcutRow {
                action: "Delete",
                windows: "Delete",
                mac: "Backspace",
            },
            ShortcutRow {
                action: "Crop image",
                windows: "C + Drag",
                mac: "C + Drag",
            },
            ShortcutRow {
                action: "Edit item",
                windows: "Return",
                mac: "↵",
            },
            ShortcutRow {
                action: "Focus canvas",
                windows: "Ctrl+Space",
                mac: "⌘Space",
            },
            ShortcutRow {
                action: "Focus image",
                windows: "Space",
                mac: "Space",
            },
            ShortcutRow {
                action: "Pan",
                windows: "Left MB + Alt",
                mac: "Left MB + ⌥",
            },
            ShortcutRow {
                action: "Select all",
                windows: "Ctrl+A",
                mac: "⌘A",
            },
            ShortcutRow {
                action: "Select items",
                windows: "Left MB",
                mac: "Left MB",
            },
            ShortcutRow {
                action: "Zoom in",
                windows: "Ctrl++",
                mac: "⌘+",
            },
            ShortcutRow {
                action: "Zoom out",
                windows: "Ctrl+-",
                mac: "⌘-",
            },
        ],
    },
    ShortcutSection {
        title: "Draw",
        rows: &[
            ShortcutRow {
                action: "Cancel drawing",
                windows: "Esc",
                mac: "⎋",
            },
            ShortcutRow {
                action: "Select line tool",
                windows: "L",
                mac: "L",
            },
            ShortcutRow {
                action: "Select pen tool",
                windows: "D",
                mac: "D",
            },
            ShortcutRow {
                action: "Select rectangle tool",
                windows: "R",
                mac: "R",
            },
            ShortcutRow {
                action: "Select eraser tool",
                windows: "E",
                mac: "E",
            },
        ],
    },
    ShortcutSection {
        title: "Group",
        rows: &[
            ShortcutRow {
                action: "Group",
                windows: "Ctrl+G",
                mac: "⌘G",
            },
            ShortcutRow {
                action: "Ungroup",
                windows: "Ctrl+Shift+G",
                mac: "⇧⌘G",
            },
        ],
    },
    ShortcutSection {
        title: "Multi Note",
        rows: &[
            ShortcutRow {
                action: "Toggle bold text",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Toggle italic text",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Toggle text underline",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Text highlight color",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Paste text formatting",
                windows: "None",
                mac: "None",
            },
        ],
    },
    ShortcutSection {
        title: "Note",
        rows: &[
            ShortcutRow {
                action: "Align text left",
                windows: "Ctrl+Shift+L",
                mac: "⇧⌘L",
            },
            ShortcutRow {
                action: "Align text center",
                windows: "Ctrl+Shift+E",
                mac: "⇧⌘E",
            },
            ShortcutRow {
                action: "Align text right",
                windows: "Ctrl+Shift+R",
                mac: "⇧⌘R",
            },
            ShortcutRow {
                action: "Decrease font size",
                windows: "Ctrl+Shift+,",
                mac: "⇧⌘,",
            },
            ShortcutRow {
                action: "Increase font size",
                windows: "Ctrl+Shift+.",
                mac: "⇧⌘.",
            },
            ShortcutRow {
                action: "Insert link",
                windows: "Ctrl+K",
                mac: "⌘K",
            },
            ShortcutRow {
                action: "Paste without formatting",
                windows: "Ctrl+Shift+V",
                mac: "⇧⌘V",
            },
            ShortcutRow {
                action: "Toggle note auto size",
                windows: "Ctrl+Shift+A",
                mac: "⇧⌘A",
            },
            ShortcutRow {
                action: "Toggle bulleted list",
                windows: "Ctrl+Shift+B",
                mac: "⇧⌘B",
            },
            ShortcutRow {
                action: "Toggle numbered list",
                windows: "Ctrl+Shift+N",
                mac: "⇧⌘N",
            },
        ],
    },
    ShortcutSection {
        title: "Playback",
        rows: &[
            ShortcutRow {
                action: "Next frame",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Play / Pause",
                windows: "None",
                mac: "None",
            },
            ShortcutRow {
                action: "Previous frame",
                windows: "None",
                mac: "None",
            },
        ],
    },
    ShortcutSection {
        title: "System",
        rows: &[
            ShortcutRow {
                action: "Move window",
                windows: "Right MB",
                mac: "Right MB",
            },
            ShortcutRow {
                action: "Open context menu",
                windows: "Right MB",
                mac: "Right MB",
            },
            ShortcutRow {
                action: "Resize window",
                windows: "Left MB",
                mac: "Left MB",
            },
        ],
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
    removed_asset_id: i64,
}

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub screenshot_path: Option<std::path::PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut screenshot_path = None;
        let mut arguments = std::env::args().skip(1);

        while let Some(argument) = arguments.next() {
            if argument == "--screenshot" {
                screenshot_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--screenshot=") {
                screenshot_path = Some(std::path::PathBuf::from(path));
            }
        }

        Self { screenshot_path }
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
    auto_image_scaling: bool,
    import_gap: f32,
    drawer_open: bool,
    status: String,
}

impl ReferenceApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        let library_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("library");

        match crate::storage::Library::initialize(library_root) {
            Ok(library) => {
                let import_worker = crate::jobs::ImportWorker::new(library.clone());
                let status = "Library ready".to_owned();
                let mut app = Self {
                    canvas: crate::canvas::CanvasState::default(),
                    screenshot_capture: crate::screenshot::ScreenshotCapture::new(
                        config.screenshot_path.clone(),
                    ),
                    window_capture:
                        crate::native_capture::NativeWindowCapture::from_creation_context(cc),
                    wgpu_render_state: cc.wgpu_render_state.clone(),
                    library: Some(library.clone()),
                    import_worker: Some(import_worker),
                    crop_undo_stack: Vec::new(),
                    auto_image_scaling: true,
                    import_gap: crate::data::DEFAULT_IMPORT_GAP,
                    drawer_open: false,
                    status,
                };
                app.load_user_session();
                app.enqueue_existing_items();
                app
            }
            Err(error) => Self {
                canvas: crate::canvas::CanvasState::default(),
                screenshot_capture: crate::screenshot::ScreenshotCapture::new(
                    config.screenshot_path,
                ),
                window_capture: crate::native_capture::NativeWindowCapture::from_creation_context(
                    cc,
                ),
                wgpu_render_state: cc.wgpu_render_state.clone(),
                library: None,
                import_worker: None,
                crop_undo_stack: Vec::new(),
                auto_image_scaling: true,
                import_gap: crate::data::DEFAULT_IMPORT_GAP,
                drawer_open: false,
                status: format!("Storage unavailable: {error}"),
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
            Ok(records) => {
                for record in records {
                    if let Err(error) =
                        import_worker.enqueue(crate::jobs::ImportRequest::LoadExisting { record })
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
            }
            Err(error) => self.status = error,
        }
    }

    fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

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

            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::ImportPath {
                source_path: source_path.clone(),
                desired_world,
                viewport_world,
                auto_scale: self.auto_image_scaling,
                import_gap: self.import_gap,
            }) {
                self.status = error;
                continue;
            }

            self.status = format!("Queued import: {}", source_path.display());
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

        let Some(import_worker) = &self.import_worker else {
            return;
        };

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

        if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::ImportClipboard {
            rgba,
            width,
            height,
            desired_world,
            viewport_world,
            auto_scale: self.auto_image_scaling,
            import_gap: self.import_gap,
        }) {
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
        if !copy_requested || !self.canvas.has_selection() {
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

        for result in import_worker.try_recv_all() {
            match result {
                crate::jobs::ImportResult::Loaded(loaded) => {
                    self.canvas.add_loaded_item(ctx, loaded);
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

                    updated_item.asset_id = asset.id;

                    let updated_item_id = updated_item.id;
                    match self
                        .canvas
                        .apply_cropped_item(ctx, cropped, asset.clone(), updated_item)
                    {
                        Ok(()) => {
                            self.push_crop_undo_entry(CropUndoEntry {
                                previous_record: previous_snapshot.0,
                                previous_rgba: previous_snapshot.1,
                                removed_asset_id: asset.id,
                            });
                            self.canvas.clear_crop_pending(updated_item_id);
                            self.status = "Cropped image".to_owned();
                        }
                        Err(error) => {
                            self.canvas.clear_crop_pending(updated_item_id);
                            self.status = error;
                        }
                    }
                }
                crate::jobs::ImportResult::Failed {
                    item_id,
                    context,
                    error,
                } => {
                    if let Some(item_id) = item_id {
                        self.canvas.clear_crop_pending(item_id);
                    }
                    self.status = format!("{context}: {error}");
                }
            }
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
            self.canvas.evict_cached_asset(entry.removed_asset_id);
        }

        self.canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                record: entry.previous_record,
                rgba: entry.previous_rgba,
            },
        );
        self.status = "Undid crop".to_owned();
    }

    fn handle_selection_shortcuts(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        for shortcut in SUPPORTED_SHORTCUTS {
            let Some(shortcut_action) = shortcut.shortcut_action else {
                continue;
            };
            let triggered = match shortcut_action {
                SupportedShortcutAction::DeleteSelection => delete_shortcut_pressed(ctx),
                SupportedShortcutAction::DuplicateSelection
                | SupportedShortcutAction::ToggleDrawer => shortcut
                    .binding
                    .is_some_and(|binding| shortcut_pressed(ctx, binding)),
                SupportedShortcutAction::Arrange(_) => shortcut
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
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.duplicate_selection_with_gap(ctx, screen_rect);
                }
                SupportedShortcutAction::ToggleDrawer => {
                    self.drawer_open = !self.drawer_open;
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
            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::Crop { request })
            {
                self.canvas.clear_crop_pending(item_id);
                self.status = error;
                continue;
            }

            self.status = "Cropping image".to_owned();
        }
    }

    fn persist_canvas_changes(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        for item in self.canvas.take_dirty_items() {
            if let Err(error) = library.update_board_item(&item) {
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
        if let Err(error) = library.save_user_session(&session) {
            self.status = error;
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
        let drawer_rect = self.drawer_rect(screen_rect);
        let visible_screen_rect = egui::Rect::from_min_max(
            screen_rect.min,
            egui::pos2(screen_rect.max.x, drawer_rect.top()),
        );
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
        let drawer_rect = self.drawer_rect(screen_rect);
        let canvas_center = egui::pos2(
            screen_rect.center().x,
            (screen_rect.top() + drawer_rect.top()) * 0.5,
        );
        let import_screen_pos = pointer_screen_pos
            .filter(|screen_pos| {
                screen_rect.contains(*screen_pos) && !drawer_rect.contains(*screen_pos)
            })
            .unwrap_or(canvas_center);
        self.canvas.screen_to_world(import_screen_pos, screen_rect)
    }

    fn drawer_height(&self, screen_rect: egui::Rect) -> f32 {
        let target = (screen_rect.height() * 0.42)
            .clamp(SHORTCUT_DRAWER_MIN_HEIGHT, SHORTCUT_DRAWER_MAX_HEIGHT);
        let available = (screen_rect.height() - 72.0).max(140.0);
        target.min(available)
    }

    fn drawer_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let height = if self.drawer_open {
            self.drawer_height(screen_rect)
        } else {
            0.0
        };
        egui::Rect::from_min_max(
            egui::pos2(
                screen_rect.left() + SHORTCUT_DRAWER_MARGIN,
                screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN - height,
            ),
            egui::pos2(
                screen_rect.right() - SHORTCUT_DRAWER_MARGIN,
                screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN,
            ),
        )
    }

    fn push_crop_undo_entry(&mut self, entry: CropUndoEntry) {
        self.crop_undo_stack.push(entry);
        if self.crop_undo_stack.len() > MAX_CROP_UNDO_ENTRIES {
            self.crop_undo_stack.remove(0);
        }
    }
}

impl eframe::App for ReferenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.ctx().viewport_rect();
        ui.set_clip_rect(screen_rect);
        ui.expand_to_include_rect(screen_rect);

        self.handle_import_results(ui.ctx());
        self.handle_crop_undo(ui.ctx());

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
        self.handle_selection_shortcuts(ui.ctx(), screen_rect);
        let drawer_open = self.drawer_open;
        let drawer_height = if drawer_open {
            self.drawer_height(screen_rect)
        } else {
            0.0
        };
        if drawer_open {
            block_drawer_input(ui.ctx(), self.drawer_rect(screen_rect));
        }
        crate::canvas::show(ui, screen_rect, &mut self.canvas, false);
        self.handle_pending_crop_requests();
        show_selection_toolbar(
            ui.ctx(),
            screen_rect,
            &mut self.canvas,
            self.library.as_ref(),
            self.import_gap,
            &mut self.status,
            drawer_height,
        );
        self.persist_canvas_changes();
        let settings_changed = show_shortcuts_drawer(
            ui.ctx(),
            screen_rect,
            drawer_open,
            &mut self.drawer_open,
            &mut self.auto_image_scaling,
            &mut self.import_gap,
        );
        self.persist_settings_if_changed(settings_changed);
        show_drawer_toggle(ui.ctx(), screen_rect, drawer_open, &mut self.drawer_open);
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
        crate::theme::SHADE_90.to_normalized_gamma_f32()
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
    let archived_ids = canvas.delete_selected();
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

fn show_selection_toolbar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    canvas: &mut crate::canvas::CanvasState,
    library: Option<&crate::storage::Library>,
    gap: f32,
    status: &mut String,
    bottom_reserved_height: f32,
) {
    if !canvas.has_selection() {
        return;
    }

    let selection_count = canvas.selected_item_ids().len();
    let Some(selection_rect) = canvas.selection_screen_rect(screen_rect) else {
        return;
    };

    let min_x = screen_rect.min.x + 8.0;
    let toolbar_width = if selection_count > 1 {
        MULTI_SELECTION_TOOLBAR_WIDTH
    } else {
        SELECTION_TOOLBAR_WIDTH
    };
    let max_x = screen_rect.max.x - toolbar_width - 8.0;
    let min_y = screen_rect.min.y + 8.0;
    let max_y =
        (screen_rect.max.y - bottom_reserved_height - SELECTION_TOOLBAR_HEIGHT - 8.0).max(min_y);

    let toolbar_x = (selection_rect.center().x - toolbar_width * 0.5).clamp(min_x, max_x);
    let toolbar_y = (selection_rect.min.y - SELECTION_TOOLBAR_HEIGHT - SELECTION_TOOLBAR_GAP)
        .clamp(min_y, max_y);

    egui::Area::new(egui::Id::new("selection_toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(toolbar_x, toolbar_y))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::SHADE_80)
                .stroke(egui::Stroke::NONE)
                .corner_radius(10)
                .inner_margin(egui::Margin {
                    left: 4,
                    right: 4,
                    top: 4,
                    bottom: 4,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let action_button = |label: &str, min_width: f32| {
                            egui::Button::new(
                                egui::RichText::new(label)
                                    .size(12.0)
                                    .color(crate::theme::SHADE_BASE),
                            )
                            .min_size(egui::vec2(min_width, 28.0))
                            .frame(true)
                            .frame_when_inactive(false)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(8)
                        };

                        if selection_count > 1 {
                            if ui.add(action_button("Compact", 66.0)).clicked() {
                                apply_arrangement_action(
                                    canvas,
                                    ArrangementAction::Compact,
                                    gap,
                                    status,
                                );
                            }

                            if ui.add(action_button("Row", 44.0)).clicked() {
                                apply_arrangement_action(
                                    canvas,
                                    ArrangementAction::Horizontal,
                                    gap,
                                    status,
                                );
                            }

                            if ui.add(action_button("Column", 62.0)).clicked() {
                                apply_arrangement_action(
                                    canvas,
                                    ArrangementAction::Vertical,
                                    gap,
                                    status,
                                );
                            }
                        }

                        let archive_icon = egui::Image::new(ARCHIVE_ICON)
                            .fit_to_exact_size(egui::vec2(16.0, 16.0))
                            .tint(crate::theme::SHADE_BASE);
                        let archive_button = egui::Button::image(archive_icon)
                            .min_size(egui::vec2(28.0, 28.0))
                            .frame(true)
                            .frame_when_inactive(false)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(8);

                        if ui.add(archive_button).clicked() {
                            delete_selected_items(canvas, library, status);
                        }
                    });
                });
        });
}

fn block_drawer_input(ctx: &egui::Context, drawer_rect: egui::Rect) {
    if drawer_rect.height() <= 0.0 || drawer_rect.width() <= 0.0 {
        return;
    }

    egui::Area::new(egui::Id::new("shortcut_drawer_blocker"))
        .order(egui::Order::Foreground)
        .fixed_pos(drawer_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(drawer_rect.size());
            ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
        });
}

fn show_shortcuts_drawer(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    drawer_open: bool,
    drawer_open_state: &mut bool,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
) -> bool {
    if !drawer_open {
        return false;
    }

    let mut settings_changed = false;

    let drawer_height = {
        let target = (screen_rect.height() * 0.42)
            .clamp(SHORTCUT_DRAWER_MIN_HEIGHT, SHORTCUT_DRAWER_MAX_HEIGHT);
        let available = (screen_rect.height() - 72.0).max(140.0);
        target.min(available)
    };
    let drawer_rect = egui::Rect::from_min_max(
        egui::pos2(
            screen_rect.left() + SHORTCUT_DRAWER_MARGIN,
            screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN - drawer_height,
        ),
        egui::pos2(
            screen_rect.right() - SHORTCUT_DRAWER_MARGIN,
            screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN,
        ),
    );

    egui::Area::new(egui::Id::new("shortcut_drawer"))
        .order(egui::Order::Foreground)
        .fixed_pos(drawer_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(drawer_rect.size());

            egui::Frame::new()
                .fill(crate::theme::SHADE_80)
                .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_70))
                .corner_radius(SHORTCUT_SECTION_CARD_RADIUS)
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 14,
                    bottom: 16,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_max_width(ui.available_width());
                    ui.set_min_height(drawer_rect.height());

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new("Shortcuts")
                                    .size(14.0)
                                    .strong()
                                    .color(crate::theme::SHADE_BASE),
                            );
                            ui.label(
                                egui::RichText::new("Implemented now + handbook reference")
                                    .size(11.0)
                                    .color(crate::theme::SHADE_30),
                            );
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close_button = egui::Button::new(
                                egui::RichText::new("×")
                                    .size(15.0)
                                    .color(crate::theme::SHADE_BASE),
                            )
                            .min_size(egui::vec2(32.0, 28.0))
                            .frame(true)
                            .frame_when_inactive(false)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(8);

                            if ui
                                .add(close_button)
                                .on_hover_text("Close shortcuts")
                                .clicked()
                            {
                                *drawer_open_state = false;
                            }
                        });
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(12.0);

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 10.0);
                            settings_changed |=
                                paint_settings_section(ui, auto_image_scaling, import_gap);
                            paint_supported_shortcuts_section(ui);
                            for section in SHORTCUT_SECTIONS {
                                paint_shortcut_section(ui, section);
                            }
                        });
                });
        });

    settings_changed
}

fn paint_settings_section(
    ui: &mut egui::Ui,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(crate::theme::SHADE_90)
        .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_70))
        .corner_radius(SHORTCUT_SECTION_CARD_RADIUS)
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Settings")
                        .size(12.0)
                        .strong()
                        .color(crate::theme::SHADE_BASE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("New images only")
                            .size(10.0)
                            .color(crate::theme::SHADE_30),
                    );
                });
            });
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Auto-scale new images")
                            .size(11.0)
                            .color(crate::theme::SHADE_BASE),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Cap imported image area at {:.0} canvas sq units",
                            crate::jobs::DEFAULT_IMPORT_TARGET_CANVAS_AREA
                        ))
                        .size(10.0)
                        .color(crate::theme::SHADE_30),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    changed |= paint_switch(ui, auto_image_scaling).changed();
                });
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Import gap")
                            .size(11.0)
                            .color(crate::theme::SHADE_BASE),
                    );
                    ui.label(
                        egui::RichText::new("Gap used when placing new images")
                            .size(10.0)
                            .color(crate::theme::SHADE_30),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    changed |= ui
                        .add(
                        egui::DragValue::new(import_gap)
                            .speed(0.5)
                            .range(0.0..=128.0)
                            .fixed_decimals(0)
                            .suffix(" units"),
                    )
                    .changed();
                });
            });
        });

    changed
}

fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SHADE_90)
        .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_70))
        .corner_radius(SHORTCUT_SECTION_CARD_RADIUS)
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Supported shortcuts")
                        .size(12.0)
                        .strong()
                        .color(crate::theme::SHADE_BASE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} actions", SUPPORTED_SHORTCUTS.len()))
                            .size(10.0)
                            .color(crate::theme::SHADE_30),
                    );
                });
            });
            ui.add_space(8.0);

            egui::Grid::new(egui::Id::new("supported_shortcuts_grid"))
                .num_columns(3)
                .striped(false)
                .spacing(egui::vec2(12.0, 6.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Action")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("Windows/Linux")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("Mac")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.end_row();

                    for shortcut in SUPPORTED_SHORTCUTS {
                        ui.label(
                            egui::RichText::new(shortcut.action)
                                .size(11.0)
                                .color(crate::theme::SHADE_BASE),
                        );
                        ui.label(
                            egui::RichText::new(shortcut.windows)
                                .size(10.0)
                                .color(crate::theme::SHADE_20),
                        );
                        ui.label(
                            egui::RichText::new(shortcut.mac)
                                .size(10.0)
                                .color(crate::theme::SHADE_20),
                        );
                        ui.end_row();
                    }
                });
        });
}

fn paint_switch(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let desired_size = egui::vec2(40.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }

    let visuals = ui.style().interact_selectable(&response, *value);
    let track_color = if *value {
        crate::theme::SHADE_BASE
    } else {
        crate::theme::SHADE_60
    };
    let knob_color = if *value {
        crate::theme::SHADE_90
    } else {
        crate::theme::SHADE_BASE
    };
    let animation = ui.ctx().animate_bool(response.id, *value);
    let knob_radius = 9.0;
    let knob_x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), animation);
    let track_rect = rect.shrink2(egui::vec2(1.0, 3.0));

    ui.painter().rect(
        track_rect,
        track_rect.height() * 0.5,
        track_color,
        egui::Stroke::new(1.0, visuals.bg_stroke.color),
        egui::StrokeKind::Middle,
    );
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), knob_radius, knob_color);

    response
}

fn show_drawer_toggle(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    drawer_open: bool,
    drawer_open_state: &mut bool,
) {
    let button_size = egui::vec2(92.0, SHORTCUT_DRAWER_BUTTON_SIZE);
    let y = if drawer_open {
        let drawer_height = {
            let target = (screen_rect.height() * 0.42)
                .clamp(SHORTCUT_DRAWER_MIN_HEIGHT, SHORTCUT_DRAWER_MAX_HEIGHT);
            let available = (screen_rect.height() - 72.0).max(140.0);
            target.min(available)
        };
        screen_rect.bottom() - drawer_height - SHORTCUT_DRAWER_MARGIN - button_size.y
    } else {
        screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN - button_size.y
    };
    let x = screen_rect.right() - SHORTCUT_DRAWER_MARGIN - button_size.x;

    egui::Area::new(egui::Id::new("shortcut_drawer_toggle"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, y))
        .show(ctx, |ui| {
            let button = egui::Button::new(
                egui::RichText::new("Command B")
                    .size(12.0)
                    .color(crate::theme::SHADE_BASE),
            )
            .min_size(button_size)
            .frame(true)
            .frame_when_inactive(false)
            .stroke(egui::Stroke::NONE)
            .corner_radius(16);

            let response = ui.add(button);
            let response = if drawer_open {
                response.on_hover_text("Close shortcuts")
            } else {
                response.on_hover_text("Open shortcuts")
            };
            if response.clicked() {
                *drawer_open_state = !*drawer_open_state;
            }
        });
}

fn paint_shortcut_section(ui: &mut egui::Ui, section: &ShortcutSection) {
    egui::Frame::new()
        .fill(crate::theme::SHADE_90)
        .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_70))
        .corner_radius(SHORTCUT_SECTION_CARD_RADIUS)
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(section.title)
                        .size(12.0)
                        .strong()
                        .color(crate::theme::SHADE_BASE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} rows", section.rows.len()))
                            .size(10.0)
                            .color(crate::theme::SHADE_30),
                    );
                });
            });
            ui.add_space(8.0);

            egui::Grid::new(egui::Id::new(("shortcut_section_grid", section.title)))
                .num_columns(3)
                .striped(false)
                .spacing(egui::vec2(12.0, 6.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Function")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("Windows/Linux")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("Mac")
                            .size(10.0)
                            .color(crate::theme::SHADE_40)
                            .strong(),
                    );
                    ui.end_row();

                    for row in section.rows {
                        ui.label(
                            egui::RichText::new(row.action)
                                .size(11.0)
                                .color(crate::theme::SHADE_10),
                        );
                        ui.label(
                            egui::RichText::new(row.windows)
                                .size(11.0)
                                .color(crate::theme::SHADE_20),
                        );
                        ui.label(
                            egui::RichText::new(row.mac)
                                .size(11.0)
                                .color(crate::theme::SHADE_20),
                        );
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::{apply_arrangement_action, ArrangementAction, ReferenceApp};
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

        app.save_user_session();

        let session = library.load_user_session().unwrap();
        assert!(!session.auto_image_scaling);
        assert_eq!(session.import_gap, 37.0);

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
            canvas.add_loaded_item(&ctx, LoadedCanvasItem { record, rgba });
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

    fn test_app(library: Option<Library>) -> ReferenceApp {
        ReferenceApp {
            canvas: crate::canvas::CanvasState::default(),
            screenshot_capture: crate::screenshot::ScreenshotCapture::new(None),
            window_capture: None,
            wgpu_render_state: None,
            library,
            import_worker: None,
            crop_undo_stack: Vec::new(),
            auto_image_scaling: true,
            import_gap: crate::data::DEFAULT_IMPORT_GAP,
            drawer_open: false,
            status: String::new(),
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
