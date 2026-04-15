use eframe::egui;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const SIDEBAR_WIDTH: f32 = 180.0;
const SIDEBAR_MAX_WIDTH: f32 = 200.0;
const SIDEBAR_CONTENT_TOP_PADDING: f32 = 46.0;
const SELECTION_TOOLBAR_WIDTH: f32 = 44.0;
const SELECTION_TOOLBAR_HEIGHT: f32 = 36.0;
const SELECTION_TOOLBAR_GAP: f32 = 12.0;
const ARCHIVE_ICON: egui::ImageSource<'static> =
    egui::include_image!("../assets/icons/iconoir-archive.svg");
const COPY_PAYLOAD_KIND: &str = "reference-board-items";

#[derive(Debug, Serialize, Deserialize)]
struct ClipboardBoardSelection {
    kind: String,
    source_center_x: f32,
    source_center_y: f32,
    item_ids: Vec<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    pub dev_mode: bool,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let dev_mode = std::env::args().skip(1).any(|argument| argument == "--dev");
        Self { dev_mode }
    }
}

pub struct ReferenceApp {
    canvas: crate::canvas::CanvasState,
    dev_harness: crate::dev_harness::DevHarness,
    window_capture: Option<crate::native_capture::NativeWindowCapture>,
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    library: Option<crate::storage::Library>,
    import_worker: Option<crate::jobs::ImportWorker>,
    archived_entries: Vec<crate::storage::ArchivedSidebarEntry>,
    archive_section_open: bool,
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
                let mut status = "Library ready".to_owned();
                let dev_harness =
                    match crate::dev_harness::DevHarness::new(config.dev_mode, library.root()) {
                        Ok(harness) => harness,
                        Err(error) => {
                            status = format!("Dev harness unavailable: {error}");
                            crate::dev_harness::DevHarness::new(false, library.root())
                                .unwrap_or_else(|_| {
                                    unreachable!("disabled dev harness should always initialize")
                                })
                        }
                    };
                let mut app = Self {
                    canvas: crate::canvas::CanvasState::default(),
                    dev_harness,
                    window_capture:
                        crate::native_capture::NativeWindowCapture::from_creation_context(cc),
                    wgpu_render_state: cc.wgpu_render_state.clone(),
                    library: Some(library.clone()),
                    import_worker: Some(import_worker),
                    archived_entries: Vec::new(),
                    archive_section_open: false,
                    status,
                };
                app.load_user_session();
                app.enqueue_existing_items();
                app.refresh_archived_entries();
                app
            }
            Err(error) => Self {
                canvas: crate::canvas::CanvasState::default(),
                dev_harness: crate::dev_harness::DevHarness::new(false, std::path::Path::new("."))
                    .unwrap_or_else(|_| {
                        unreachable!("disabled dev harness should always initialize")
                    }),
                window_capture: crate::native_capture::NativeWindowCapture::from_creation_context(
                    cc,
                ),
                wgpu_render_state: cc.wgpu_render_state.clone(),
                library: None,
                import_worker: None,
                archived_entries: Vec::new(),
                archive_section_open: false,
                status: format!("Storage unavailable: {error}"),
            },
        }
    }

    fn refresh_archived_entries(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_archived_sidebar_entries() {
            Ok(entries) => {
                self.archived_entries = entries;
            }
            Err(error) => {
                self.status = error;
            }
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
            Ok(session) => self.canvas.apply_user_session(&session),
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
                crate::jobs::ImportResult::Failed { context, error } => {
                    self.status = format!("{context}: {error}");
                }
            }
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

        if let Err(error) = library.save_user_session(&self.canvas.user_session()) {
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
        let sidebar_rect = sidebar_rect(screen_rect);
        let import_screen_pos = pointer_screen_pos
            .filter(|screen_pos| {
                screen_rect.contains(*screen_pos) && !sidebar_rect.contains(*screen_pos)
            })
            .unwrap_or(screen_rect.center());
        self.canvas.screen_to_world(import_screen_pos, screen_rect)
    }
}

impl eframe::App for ReferenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.ctx().viewport_rect();
        ui.set_clip_rect(screen_rect);
        ui.expand_to_include_rect(screen_rect);

        self.handle_import_results(ui.ctx());

        if self.dev_harness.has_pending_screenshot()
            && let Some(render_state) = &self.wgpu_render_state
        {
            let _ = render_state.device.poll(eframe::wgpu::PollType::Poll);
            ui.ctx().request_repaint();
        }

        if self.dev_harness.is_enabled() {
            match self.dev_harness.begin_frame(
                ui.ctx(),
                screen_rect,
                &mut self.canvas,
                self.window_capture.as_ref(),
                self.library.as_ref(),
                &mut self.status,
            ) {
                crate::dev_harness::DevHarnessAction::None => {}
                crate::dev_harness::DevHarnessAction::CloseApp => {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        self.handle_clipboard_copy(ui.ctx());
        self.handle_clipboard_paste(ui.ctx(), screen_rect);
        self.handle_dropped_files(ui, screen_rect);
        crate::canvas::show(
            ui,
            screen_rect,
            &mut self.canvas,
            self.dev_harness.show_overlay(),
        );
        show_selection_toolbar(
            ui.ctx(),
            screen_rect,
            &mut self.canvas,
            self.library.as_ref(),
            &mut self.archived_entries,
            &mut self.status,
        );
        self.persist_canvas_changes();
        show_sidebar(
            ui.ctx(),
            screen_rect,
            &mut self.canvas,
            &self.archived_entries,
            &mut self.archive_section_open,
            &mut self.status,
        );
        self.dev_harness
            .show_overlay_ui(ui.ctx(), screen_rect, &self.canvas, &self.status);
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
    archived_entries: &mut Vec<crate::storage::ArchivedSidebarEntry>,
    status: &mut String,
) {
    if !canvas.has_selection() {
        return;
    }

    let Some(selection_rect) = canvas.selection_screen_rect(screen_rect) else {
        return;
    };

    let canvas_rect = canvas_view_rect(screen_rect);
    let min_x = canvas_rect.min.x + 8.0;
    let max_x = canvas_rect.max.x - SELECTION_TOOLBAR_WIDTH - 8.0;
    let min_y = canvas_rect.min.y + 8.0;
    let max_y = canvas_rect.max.y - SELECTION_TOOLBAR_HEIGHT - 8.0;

    let toolbar_x = (selection_rect.center().x - SELECTION_TOOLBAR_WIDTH * 0.5).clamp(min_x, max_x);
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
                                match library.load_archived_sidebar_entries() {
                                    Ok(entries) => *archived_entries = entries,
                                    Err(error) => {
                                        *status = error;
                                        return;
                                    }
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

fn show_sidebar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    canvas: &mut crate::canvas::CanvasState,
    archived_entries: &[crate::storage::ArchivedSidebarEntry],
    archive_section_open: &mut bool,
    status: &mut String,
) {
    let sidebar_height = screen_rect.height();
    let entries = canvas.sidebar_entries();

    egui::Area::new(egui::Id::new("library_sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(screen_rect.left(), screen_rect.top()))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::SHADE_80)
                .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_70))
                .corner_radius(0)
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 12,
                    top: (SIDEBAR_CONTENT_TOP_PADDING + 8.0) as i8,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.set_width(SIDEBAR_WIDTH);
                    ui.set_min_width(SIDEBAR_WIDTH);
                    ui.set_max_width(SIDEBAR_MAX_WIDTH);
                    ui.set_min_height(sidebar_height);

                    ui.vertical(|ui| {
                        paint_sidebar_header(ui, entries.len());
                        ui.add_space(12.0);
                        paint_sidebar_divider(ui);
                        ui.add_space(10.0);

                        let archive_section_height = archive_section_target_height(
                            *archive_section_open,
                            archived_entries.len(),
                        );
                        let status_height = 28.0;
                        let active_height =
                            (ui.available_height() - archive_section_height - status_height - 18.0)
                                .max(80.0);

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), active_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
                                        paint_library_tree(
                                            ui,
                                            screen_rect,
                                            canvas,
                                            status,
                                            &entries,
                                        );
                                    });
                            },
                        );

                        egui::ScrollArea::vertical()
                            .max_height(archive_section_height)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                paint_archive_section(ui, archive_section_open, archived_entries);
                            });

                        ui.add_space(10.0);
                        paint_sidebar_divider(ui);
                        ui.add_space(8.0);
                        ui.add_sized(
                            [ui.available_width(), 18.0],
                            egui::Label::new(
                                egui::RichText::new(status.as_str())
                                    .size(12.0)
                                    .color(crate::theme::SHADE_30),
                            )
                            .truncate(),
                        );
                    });
                });
        });
}

fn sidebar_rect(screen_rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(screen_rect.left(), screen_rect.top()),
        egui::vec2(SIDEBAR_WIDTH, screen_rect.height()),
    )
}

fn canvas_view_rect(screen_rect: egui::Rect) -> egui::Rect {
    let sidebar = sidebar_rect(screen_rect);
    egui::Rect::from_min_max(
        egui::pos2(sidebar.max.x, screen_rect.min.y),
        screen_rect.max,
    )
}

fn paint_sidebar_header(ui: &mut egui::Ui, entry_count: usize) {
    let noun = if entry_count == 1 { "item" } else { "items" };
    ui.label(
        egui::RichText::new(format!("{entry_count} {noun}"))
            .size(12.0)
            .color(crate::theme::SHADE_30),
    );
}

fn archive_section_target_height(open: bool, count: usize) -> f32 {
    if !open {
        return 28.0;
    }

    let rows = count.min(5) as f32;
    28.0 + 8.0 + rows * 24.0
}

fn paint_sidebar_divider(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.min.y;
    ui.painter().line_segment(
        [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
        egui::Stroke::new(1.0, crate::theme::SHADE_70),
    );
    ui.add_space(1.0);
}

fn paint_archive_section(
    ui: &mut egui::Ui,
    open: &mut bool,
    entries: &[crate::storage::ArchivedSidebarEntry],
) {
    paint_sidebar_divider(ui);
    ui.add_space(8.0);

    let archive_icon = egui::Image::new(ARCHIVE_ICON)
        .fit_to_exact_size(egui::vec2(14.0, 14.0))
        .tint(crate::theme::SHADE_20);
    let chevron = if *open { "v" } else { ">" };
    let noun = if entries.len() == 1 { "item" } else { "items" };

    ui.horizontal(|ui| {
        let header = egui::Button::image_and_text(archive_icon, format!("Archive"))
            .frame(false)
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(6);
        if ui.add(header).clicked() {
            *open = !*open;
        }

        ui.label(
            egui::RichText::new(chevron)
                .size(11.0)
                .color(crate::theme::SHADE_40),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} {noun}", entries.len()))
                    .size(10.0)
                    .color(crate::theme::SHADE_40),
            );
        });
    });

    if !*open {
        return;
    }

    ui.add_space(8.0);
    for entry in entries {
        ui.add_sized(
            [ui.available_width(), 18.0],
            egui::Label::new(
                egui::RichText::new(&entry.filename)
                    .size(11.0)
                    .color(crate::theme::SHADE_30),
            )
            .truncate(),
        );
    }
}

fn paint_library_tree(
    ui: &mut egui::Ui,
    screen_rect: egui::Rect,
    canvas: &mut crate::canvas::CanvasState,
    status: &mut String,
    entries: &[crate::canvas::SidebarEntry],
) {
    let mut groups: Vec<(String, Vec<&crate::canvas::SidebarEntry>)> = Vec::new();

    for entry in entries {
        let group = format_import_date(entry.imported_at);
        match groups.last_mut() {
            Some((current_group, grouped_entries)) if current_group == &group => {
                grouped_entries.push(entry)
            }
            _ => groups.push((group, vec![entry])),
        }
    }

    for (index, (group, grouped_entries)) in groups.iter().enumerate() {
        if index > 0 {
            ui.add_space(12.0);
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(group)
                    .size(11.0)
                    .color(crate::theme::SHADE_20)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let noun = if grouped_entries.len() == 1 {
                    "file"
                } else {
                    "files"
                };
                ui.label(
                    egui::RichText::new(format!("{} {noun}", grouped_entries.len()))
                        .size(10.0)
                        .color(crate::theme::SHADE_40),
                );
            });
        });
        ui.add_space(4.0);

        for entry in grouped_entries {
            paint_sidebar_entry(ui, screen_rect, canvas, status, entry);
        }
    }
}

fn paint_sidebar_entry(
    ui: &mut egui::Ui,
    screen_rect: egui::Rect,
    canvas: &mut crate::canvas::CanvasState,
    status: &mut String,
    entry: &crate::canvas::SidebarEntry,
) {
    let row_fill = if entry.selected {
        crate::theme::SHADE_70
    } else {
        crate::theme::SHADE_80
    };
    let row_stroke = if entry.selected {
        egui::Stroke::new(1.0, crate::theme::SHADE_60)
    } else {
        egui::Stroke::NONE
    };
    let arrow_color = if entry.selected {
        crate::theme::SHADE_20
    } else {
        crate::theme::SHADE_40
    };
    let text_color = if entry.selected {
        crate::theme::SHADE_BASE
    } else {
        crate::theme::SHADE_10
    };

    egui::Frame::new()
        .fill(row_fill)
        .stroke(row_stroke)
        .corner_radius(0)
        .inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(">").size(10.0).color(arrow_color));
                ui.add_space(2.0);
                let response = ui.add_sized(
                    [ui.available_width(), 16.0],
                    egui::Button::new(
                        egui::RichText::new(&entry.filename)
                            .size(12.0)
                            .color(text_color),
                    )
                    .frame(false)
                    .wrap_mode(egui::TextWrapMode::Truncate),
                );

                if response.clicked() {
                    canvas.select_items(&[entry.item_id]);
                    if let Err(error) =
                        canvas.focus_item(entry.item_id, canvas_view_rect(screen_rect), None)
                    {
                        *status = error;
                    } else {
                        *status = format!("Focused {}", entry.filename);
                    }
                }
            });
        });
}

fn format_import_date(imported_at: i64) -> String {
    OffsetDateTime::from_unix_timestamp(imported_at)
        .map(|timestamp| timestamp.date().to_string())
        .unwrap_or_else(|_| "unknown-date".to_owned())
}
