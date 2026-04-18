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
                action: "Arrange optimal",
                windows: "Ctrl+P",
                mac: "⌘P",
            },
            ShortcutRow {
                action: "Copy",
                windows: "Ctrl+C",
                mac: "⌘C",
            },
            ShortcutRow {
                action: "Duplicate",
                windows: "Ctrl+D",
                mac: "⌘D",
            },
            ShortcutRow {
                action: "Paste",
                windows: "Ctrl+V",
                mac: "⌘V",
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
                windows: "Del",
                mac: "⌦",
            },
            ShortcutRow {
                action: "Crop image",
                windows: "C + Left MB",
                mac: "C + Left MB",
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
                action: "Move item",
                windows: "Left MB",
                mac: "Left MB",
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

        for dropped_file in dropped_files {
            let Some(source_path) = dropped_file.path else {
                continue;
            };

            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::ImportPath {
                source_path: source_path.clone(),
                desired_world,
                auto_scale: self.auto_image_scaling,
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
        let rgba = clipboard_image.bytes.into_owned();
        let width = clipboard_image.width as u32;
        let height = clipboard_image.height as u32;

        if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::ImportClipboard {
            rgba,
            width,
            height,
            desired_world,
            auto_scale: self.auto_image_scaling,
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
            input.modifiers.command
                && !input.modifiers.shift
                && input.key_pressed(egui::Key::Z)
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

    fn handle_pending_crop_requests(&mut self) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_crop() {
            let item_id = request.item_id;
            self.canvas.mark_crop_pending(item_id);
            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::Crop { request }) {
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
        if let Err(error) = library.save_user_session(&session) {
            self.status = error;
        }
    }

    fn preferred_import_world(&self, ctx: &egui::Context, screen_rect: egui::Rect) -> egui::Pos2 {
        let pointer_screen_pos = ctx.input(|input| input.pointer.latest_pos());
        self.preferred_import_world_with_pointer(screen_rect, pointer_screen_pos)
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
            &mut self.status,
            drawer_height,
        );
        self.persist_canvas_changes();
        show_shortcuts_drawer(
            ui.ctx(),
            screen_rect,
            drawer_open,
            &mut self.drawer_open,
            &mut self.auto_image_scaling,
        );
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

fn show_selection_toolbar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    canvas: &mut crate::canvas::CanvasState,
    library: Option<&crate::storage::Library>,
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
                                let arranged = canvas.arrange_selected_compact();
                                let noun = if arranged == 1 { "image" } else { "images" };
                                *status = format!("Compacted {arranged} {noun}");
                            }

                            if ui.add(action_button("Row", 44.0)).clicked() {
                                let arranged = canvas.arrange_selected_horizontal();
                                let noun = if arranged == 1 { "image" } else { "images" };
                                *status = format!("Arranged {arranged} {noun} in a row");
                            }

                            if ui.add(action_button("Column", 62.0)).clicked() {
                                let arranged = canvas.arrange_selected_vertical();
                                let noun = if arranged == 1 { "image" } else { "images" };
                                *status = format!("Arranged {arranged} {noun} in a column");
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
                            let archived_ids = canvas.delete_selected();
                            if archived_ids.is_empty() {
                                return;
                            }

                            if let Some(library) = library {
                                if let Err(error) = library.archive_board_items(&archived_ids) {
                                    *status = error;
                                    return;
                                }
                            }

                            let noun = if archived_ids.len() == 1 {
                                "image"
                            } else {
                                "images"
                            };
                            *status = format!("Archived {} {noun}", archived_ids.len());
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
) {
    if !drawer_open {
        return;
    }

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
                                egui::RichText::new("PureRef handbook defaults")
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
                            paint_settings_section(ui, auto_image_scaling);
                            for section in SHORTCUT_SECTIONS {
                                paint_shortcut_section(ui, section);
                            }
                        });
                });
        });
}

fn paint_settings_section(ui: &mut egui::Ui, auto_image_scaling: &mut bool) {
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
                            "Cap imported image height at {:.0} canvas units",
                            crate::jobs::DEFAULT_IMPORT_TARGET_CANVAS_HEIGHT
                        ))
                        .size(10.0)
                        .color(crate::theme::SHADE_30),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(auto_image_scaling, "");
                });
            });
        });
}

fn show_drawer_toggle(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    drawer_open: bool,
    drawer_open_state: &mut bool,
) {
    let button_size = SHORTCUT_DRAWER_BUTTON_SIZE;
    let y = if drawer_open {
        let drawer_height = {
            let target = (screen_rect.height() * 0.42)
                .clamp(SHORTCUT_DRAWER_MIN_HEIGHT, SHORTCUT_DRAWER_MAX_HEIGHT);
            let available = (screen_rect.height() - 72.0).max(140.0);
            target.min(available)
        };
        screen_rect.bottom() - drawer_height - SHORTCUT_DRAWER_MARGIN - button_size
    } else {
        screen_rect.bottom() - SHORTCUT_DRAWER_MARGIN - button_size
    };
    let x = screen_rect.right() - SHORTCUT_DRAWER_MARGIN - button_size;

    egui::Area::new(egui::Id::new("shortcut_drawer_toggle"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, y))
        .show(ctx, |ui| {
            let label = if drawer_open { "×" } else { "▲" };
            let button = egui::Button::new(
                egui::RichText::new(label)
                    .size(16.0)
                    .color(crate::theme::SHADE_BASE),
            )
            .min_size(egui::vec2(button_size, button_size))
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
