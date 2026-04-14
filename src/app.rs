use eframe::egui;

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
}

impl eframe::App for ReferenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.max_rect();

        self.handle_import_results(ui.ctx());
        self.handle_dropped_files(ui, screen_rect);
        crate::canvas::show(ui, screen_rect, &mut self.canvas);
        self.persist_canvas_changes();
        show_top_bar(ui.ctx(), screen_rect, &self.status);
    }
}

fn show_top_bar(ctx: &egui::Context, screen_rect: egui::Rect, status: &str) {
    let margin = 16.0;
    let bar_width = (screen_rect.width() - margin * 2.0).max(240.0);

    egui::Area::new(egui::Id::new("top_bar_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.left() + margin,
            screen_rect.top() + margin,
        ))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_premultiplied(18, 22, 30, 230))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_premultiplied(110, 126, 160, 120),
                ))
                .corner_radius(14)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width(bar_width);
                    ui.horizontal(|ui| {
                        ui.heading("Reference");
                        ui.add_space(12.0);
                        let _ = ui.button("Import");
                        ui.add_space(16.0);
                        ui.label(status);
                    });
                });
        });
}
