use eframe::egui;
use time::OffsetDateTime;

pub struct ReferenceApp {
    canvas: crate::canvas::CanvasState,
    library: Option<crate::storage::Library>,
    import_worker: Option<crate::jobs::ImportWorker>,
    status: String,
}

impl ReferenceApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let library_root = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("library");

        match crate::storage::Library::initialize(library_root) {
            Ok(library) => {
                let import_worker = crate::jobs::ImportWorker::new(library.clone());
                let mut app = Self {
                    canvas: crate::canvas::CanvasState::default(),
                    library: Some(library.clone()),
                    import_worker: Some(import_worker),
                    status: format!("Library: {}", library.root().display()),
                };
                app.load_user_session();
                app.enqueue_existing_items();
                app
            }
            Err(error) => Self {
                canvas: crate::canvas::CanvasState::default(),
                library: None,
                import_worker: None,
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

        let desired_world = self.canvas.preferred_drop_world(screen_rect);

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

    fn handle_import_results(&mut self, ctx: &egui::Context) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        for result in import_worker.try_recv_all() {
            match result {
                crate::jobs::ImportResult::Loaded(loaded) => {
                    self.canvas.add_loaded_item(ctx, loaded);
                    self.status = "Imported PNG".to_owned();
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
}

impl eframe::App for ReferenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.max_rect();

        self.handle_import_results(ui.ctx());
        self.handle_dropped_files(ui, screen_rect);
        crate::canvas::show(ui, screen_rect, &mut self.canvas);
        self.persist_canvas_changes();
        show_sidebar(
            ui.ctx(),
            screen_rect,
            &self.status,
            &self.canvas.sidebar_entries(),
        );
    }

    fn on_exit(&mut self) {
        self.save_user_session();
    }
}

fn show_sidebar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    status: &str,
    entries: &[crate::canvas::SidebarEntry],
) {
    let sidebar_width = 180.0;
    let sidebar_max_width = 200.0;
    let sidebar_height = screen_rect.height();

    egui::Area::new(egui::Id::new("library_sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(screen_rect.left(), screen_rect.top()))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::SHADE_80)
                .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_60))
                .corner_radius(0)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.set_width(sidebar_width);
                    ui.set_min_width(sidebar_width);
                    ui.set_max_width(sidebar_max_width);
                    ui.set_min_height(sidebar_height);

                    ui.vertical(|ui| {
                        ui.colored_label(crate::theme::SHADE_BASE, "Library");
                        ui.colored_label(crate::theme::SHADE_30, "./library/originals");
                        ui.add_space(8.0);

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                paint_library_tree(ui, entries);
                            });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.colored_label(crate::theme::SHADE_20, status);
                    });
                });
        });
}

fn paint_library_tree(ui: &mut egui::Ui, entries: &[crate::canvas::SidebarEntry]) {
    ui.horizontal(|ui| {
        ui.colored_label(crate::theme::SHADE_10, "v");
        ui.colored_label(crate::theme::SHADE_10, "originals");
    });
    ui.add_space(4.0);

    let mut current_group: Option<String> = None;

    for entry in entries {
        let group = format_import_date(entry.imported_at);
        if current_group.as_ref() != Some(&group) {
            current_group = Some(group.clone());
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.colored_label(crate::theme::SHADE_20, "v");
                ui.colored_label(crate::theme::SHADE_20, group);
            });
        }

        ui.horizontal(|ui| {
            ui.add_space(28.0);
            ui.colored_label(crate::theme::SHADE_40, ">");
            let color = if entry.selected {
                crate::theme::SHADE_BASE
            } else {
                crate::theme::SHADE_10
            };
            ui.colored_label(color, &entry.filename);
        });
    }
}

fn format_import_date(imported_at: i64) -> String {
    OffsetDateTime::from_unix_timestamp(imported_at)
        .map(|timestamp| timestamp.date().to_string())
        .unwrap_or_else(|_| "unknown-date".to_owned())
}
