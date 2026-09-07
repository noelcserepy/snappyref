//! Perf log and scripted perf scenarios (`--perf-scenario`), including the
//! embedding-phase timing summary written at the end of a run.

use eframe::egui;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::jobs::ImportSource;

use super::AppConfig;
use super::SnappyRefApp;

#[derive(Clone, Debug)]
pub(super) struct PerfLogState {
    pub(super) app_started_at: Instant,
    pub(super) startup_logged: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FramePerfState {
    pub(super) import_results_drained: usize,
    pub(super) import_result_drain_ms: f64,
    pub(super) canvas: crate::canvas::CanvasFrameMetrics,
}

#[derive(Clone, Debug)]
pub(super) struct PerfCorpusAsset {
    pub(super) file: String,
    pub(super) format: String,
    pub(super) animated: bool,
    pub(super) bytes: Option<u64>,
}

#[derive(Debug)]
pub(super) struct PerfScenarioState {
    pub(super) name: String,
    pub(super) corpus_root: PathBuf,
    pub(super) run_dir: PathBuf,
    pub(super) events: File,
    pub(super) started_at: Instant,
    pub(super) queued: bool,
    pub(super) expected_imports: usize,
    pub(super) loaded_imports: usize,
    pub(super) visible_imports: usize,
    /// Like the search controller, count one outstanding request per Asset.
    pub(super) pending_embedding_asset_ids: std::collections::HashSet<i64>,
    pub(super) demand_embeddings_queued: usize,
    pub(super) demand_embeddings_done: usize,
    pub(super) first_loaded_logged: bool,
    pub(super) all_loaded_logged: bool,
    pub(super) first_visible_logged: bool,
    first_frame_logged: bool,
    pub(super) startup_loaded_placement_ids: std::collections::HashSet<i64>,
    pub(super) all_visible_logged: bool,
    pub(super) first_embedding_queued_logged: bool,
    pub(super) first_embedding_done_logged: bool,
    pub(super) all_embeddings_done_logged: bool,
    pub(super) embedding_phases: EmbeddingPhaseMarkers,
    pub(super) idle_since: Option<Instant>,
    pub(super) settled_idle_at: Option<Instant>,
    pub(super) model_unload_logged: bool,
    /// Per-asset corpus metadata indexed by import queue id, so events
    /// emitted after `import_queued` can still report `format`.
    pub(super) asset_by_queue_id: std::collections::HashMap<u64, PerfCorpusAsset>,
    /// Per-asset corpus metadata indexed by stored asset id, used to
    /// stamp `format` onto embedding events once an asset id is known.
    pub(super) asset_by_asset_id: std::collections::HashMap<i64, PerfCorpusAsset>,
    /// `trickle-import` only: entries still to enqueue, popped one per
    /// `trickle_interval` to reproduce **Embedding Helper Process** churn.
    pub(super) pending_trickle_imports: Vec<PerfCorpusEntry>,
    /// Spacing between successive trickle imports. The default value
    /// keeps the bug-reproduction precise: longer than the previous
    /// "drop helper immediately" behavior so the second import always
    /// reloads, but shorter than the new 3s grace so post-fix the
    /// helper stays alive.
    pub(super) trickle_interval: Duration,
    /// Earliest instant at which the next trickle import should be
    /// enqueued; `None` once the trickle queue is empty.
    pub(super) trickle_next_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub(super) struct ModelLoadTimingSummary {
    pub(super) assets_resolved_ms: f64,
    pub(super) tokenizer_loaded_ms: f64,
    pub(super) text_session_loaded_ms: f64,
    pub(super) vision_session_loaded_ms: f64,
    pub(super) total_ms: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct EmbeddingPhaseMarkers {
    pub(super) first_embedding_queued_ms: Option<f64>,
    pub(super) semantic_model_load_started_ms: Option<f64>,
    pub(super) semantic_model_ready_ms: Option<f64>,
    pub(super) first_embedding_started_ms: Option<f64>,
    pub(super) first_embedding_done_ms: Option<f64>,
    pub(super) all_embeddings_done_ms: Option<f64>,
    pub(super) semantic_model_unload_requested_ms: Option<f64>,
    pub(super) semantic_model_unloaded_ms: Option<f64>,
    pub(super) model_load_timing: Option<ModelLoadTimingSummary>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct EmbeddingPhaseSummary {
    pub(super) queue_to_model_load_started_ms: Option<f64>,
    pub(super) model_cold_mount_ms: Option<f64>,
    pub(super) first_embedding_start_after_model_ready_ms: Option<f64>,
    pub(super) first_embedding_after_model_ready_ms: Option<f64>,
    pub(super) first_embedding_run_ms: Option<f64>,
    pub(super) total_import_embedding_drain_ms: Option<f64>,
    pub(super) model_unload_ms: Option<f64>,
    pub(super) model_load_timing: Option<ModelLoadTimingSummary>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub(super) struct PerfCorpusEntry {
    pub(super) file: String,
    pub(super) format: String,
    pub(super) role: String,
    #[serde(default)]
    pub(super) animated: bool,
    #[serde(default)]
    pub(super) animation_control: bool,
}

impl PerfScenarioState {
    /// `None` when no scenario was requested. A requested scenario that
    /// cannot start is reported on stderr and the app runs without it.
    pub(super) fn from_config(config: &AppConfig) -> Option<Self> {
        let name = config.perf_scenario.clone()?;
        match Self::start(name, config) {
            Ok(state) => Some(state),
            Err(error) => {
                eprintln!("perf scenario disabled: {error}");
                None
            }
        }
    }

    fn start(name: String, config: &AppConfig) -> Result<Self, String> {
        let corpus_root = config
            .perf_corpus
            .clone()
            .ok_or_else(|| "--perf-corpus is required with --perf-scenario".to_owned())?;
        let run_dir = config
            .perf_run_dir
            .clone()
            .ok_or_else(|| "--perf-run-dir is required with --perf-scenario".to_owned())?;
        std::fs::create_dir_all(&run_dir).map_err(|error| {
            format!(
                "failed to create perf run dir {}: {error}",
                run_dir.display()
            )
        })?;
        let events_path = run_dir.join("events.ndjson");
        let events = File::create(&events_path)
            .map_err(|error| format!("failed to create {}: {error}", events_path.display()))?;
        let mut state = Self {
            name,
            corpus_root,
            run_dir,
            events,
            started_at: Instant::now(),
            queued: false,
            expected_imports: 0,
            loaded_imports: 0,
            visible_imports: 0,
            pending_embedding_asset_ids: Default::default(),
            demand_embeddings_queued: 0,
            demand_embeddings_done: 0,
            first_loaded_logged: false,
            all_loaded_logged: false,
            first_visible_logged: false,
            first_frame_logged: false,
            startup_loaded_placement_ids: Default::default(),
            all_visible_logged: false,
            first_embedding_queued_logged: false,
            first_embedding_done_logged: false,
            all_embeddings_done_logged: false,
            embedding_phases: EmbeddingPhaseMarkers::default(),
            idle_since: None,
            settled_idle_at: None,
            model_unload_logged: false,
            asset_by_queue_id: std::collections::HashMap::new(),
            asset_by_asset_id: std::collections::HashMap::new(),
            pending_trickle_imports: Vec::new(),
            trickle_interval: Duration::from_secs(2),
            trickle_next_at: None,
        };
        state.emit("scenario_clock", serde_json::json!({}));
        Ok(state)
    }

    fn elapsed_ms(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64() * 1000.0
    }

    pub(super) fn emit(&mut self, event: &str, mut extra: serde_json::Value) {
        let elapsed_ms = self.elapsed_ms();
        self.embedding_phases.record_event(event, elapsed_ms);
        if let Some(object) = extra.as_object_mut() {
            // Align external process samples with the app's Instant-based clock.
            object.insert(
                "unix_ms".to_owned(),
                serde_json::json!(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64()
                        * 1000.0
                ),
            );
            object.insert("event".to_owned(), serde_json::json!(event));
            object.insert("elapsed_ms".to_owned(), serde_json::json!(elapsed_ms));
            object.insert("scenario".to_owned(), serde_json::json!(self.name));
        }
        let _ = writeln!(self.events, "{extra}");
        let _ = self.events.flush();
    }

    fn emit_model_load_timing(&mut self, timing: ModelLoadTimingSummary) {
        self.embedding_phases
            .model_load_timing
            .get_or_insert(timing);
        self.emit(
            "semantic_model_load_timing",
            serde_json::json!({
                "assets_resolved_ms": timing.assets_resolved_ms,
                "tokenizer_loaded_ms": timing.tokenizer_loaded_ms,
                "text_session_loaded_ms": timing.text_session_loaded_ms,
                "vision_session_loaded_ms": timing.vision_session_loaded_ms,
                "total_ms": timing.total_ms,
            }),
        );
    }

    fn manifest_path(&self) -> PathBuf {
        self.corpus_root
            .parent()
            .map(|root| root.join("manifest.json"))
            .unwrap_or_else(|| PathBuf::from("perf-assets/manifest.json"))
    }

    fn selected_entries(&self) -> Result<Vec<PerfCorpusEntry>, String> {
        let manifest_path = self.manifest_path();
        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
        let entries: Vec<PerfCorpusEntry> = serde_json::from_str(&manifest)
            .map_err(|error| format!("failed to parse {}: {error}", manifest_path.display()))?;
        let mut selected = match self.name.as_str() {
            "import-1" => entries
                .into_iter()
                .filter(|entry| entry.role.contains("default-1-image"))
                .take(1)
                .collect::<Vec<_>>(),
            "trickle-import" => {
                // Three copies of the same medium-JPEG entry used by
                // `import-1`, dripped out 2s apart by `drive_perf_scenario`
                // to reproduce **Embedding Helper Process** churn.
                let entry = entries
                    .into_iter()
                    .find(|entry| entry.role.contains("default-1-image"))
                    .ok_or_else(|| {
                        "trickle-import requires a default-1-image entry in the corpus manifest"
                            .to_owned()
                    })?;
                vec![entry.clone(), entry.clone(), entry]
            }
            "import-20" | "mixed-corpus" => entries,
            "animated" => entries
                .into_iter()
                .filter(|entry| entry.animated)
                .collect::<Vec<_>>(),
            "animated-control" => entries
                .into_iter()
                .filter(|entry| entry.animation_control)
                .collect::<Vec<_>>(),
            "static" => entries
                .into_iter()
                .filter(|entry| !entry.animated)
                .collect::<Vec<_>>(),
            "static-20" => {
                let static_entries = entries
                    .into_iter()
                    .filter(|entry| !entry.animated)
                    .collect::<Vec<_>>();
                static_entries
                    .iter()
                    .cycle()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
            }
            name if name.starts_with("format:") => {
                let format = name.trim_start_matches("format:");
                entries
                    .into_iter()
                    .filter(|entry| entry.format == format)
                    .collect::<Vec<_>>()
            }
            other => return Err(format!("unknown perf scenario: {other}")),
        };
        selected.sort_by(|left, right| left.file.cmp(&right.file));
        if selected.is_empty() {
            return Err(format!("perf scenario selected no files: {}", self.name));
        }
        Ok(selected)
    }

    fn write_summary(
        &mut self,
        worker_stats: crate::jobs::ImagePrepWorkerStats,
        image_pass: crate::canvas::ImagePassMetrics,
    ) {
        let summary = serde_json::json!({
            "scenario": self.name,
            "time_to_idle_ms": self.elapsed_ms(),
            "expected_imports": self.expected_imports,
            "loaded_imports": self.loaded_imports,
            "visible_imports": self.visible_imports,
            "demand_embeddings_queued": self.demand_embeddings_queued,
            "demand_embeddings_done": self.demand_embeddings_done,
            "embedding_phases": self.embedding_phases.summary(),
            "import_worker": {
                "queued_startup": worker_stats.queued_startup,
                "queued_interactive": worker_stats.queued_interactive,
                "queued_background": worker_stats.queued_background,
                "in_flight": worker_stats.in_flight,
            },
            "last_canvas_frame": {
                "visible_images": image_pass.visible_images,
                "painted_images": image_pass.painted_images,
                "visible_animated_images": image_pass.visible_animated_images,
                "active_animated_images": image_pass.active_animated_images,
                "fallback_images": image_pass.fallback_images,
                "draw_calls": image_pass.draw_calls,
                "texture_groups": image_pass.texture_groups,
                "build_time_ms": image_pass.build_time_ms,
            },
        });
        let path = self.run_dir.join("summary.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        );
    }
}

impl EmbeddingPhaseMarkers {
    fn record_event(&mut self, event: &str, elapsed_ms: f64) {
        match event {
            "first_import_embedding_queued" => {
                self.first_embedding_queued_ms.get_or_insert(elapsed_ms);
            }
            "semantic_model_load_started" => {
                self.semantic_model_load_started_ms
                    .get_or_insert(elapsed_ms);
            }
            "semantic_model_ready" => {
                self.semantic_model_ready_ms.get_or_insert(elapsed_ms);
            }
            "first_import_embedding_started" => {
                self.first_embedding_started_ms.get_or_insert(elapsed_ms);
            }
            "first_import_embedding_done" => {
                self.first_embedding_done_ms.get_or_insert(elapsed_ms);
            }
            "all_import_embeddings_done" => {
                self.all_embeddings_done_ms.get_or_insert(elapsed_ms);
            }
            "semantic_model_unload_requested" => {
                self.semantic_model_unload_requested_ms
                    .get_or_insert(elapsed_ms);
            }
            "semantic_model_unloaded" => {
                self.semantic_model_unloaded_ms.get_or_insert(elapsed_ms);
            }
            _ => {}
        }
    }

    pub(super) fn summary(&self) -> Option<EmbeddingPhaseSummary> {
        let has_embedding_phase = self.first_embedding_queued_ms.is_some()
            || self.semantic_model_load_started_ms.is_some()
            || self.semantic_model_ready_ms.is_some()
            || self.first_embedding_started_ms.is_some()
            || self.first_embedding_done_ms.is_some()
            || self.all_embeddings_done_ms.is_some()
            || self.model_load_timing.is_some();
        has_embedding_phase.then(|| EmbeddingPhaseSummary {
            queue_to_model_load_started_ms: duration_between(
                self.first_embedding_queued_ms,
                self.semantic_model_load_started_ms,
            ),
            model_cold_mount_ms: self
                .model_load_timing
                .map(|timing| timing.total_ms)
                .or_else(|| {
                    duration_between(
                        self.semantic_model_load_started_ms,
                        self.semantic_model_ready_ms,
                    )
                }),
            first_embedding_start_after_model_ready_ms: duration_between(
                self.semantic_model_ready_ms,
                self.first_embedding_started_ms,
            ),
            first_embedding_after_model_ready_ms: duration_between(
                self.semantic_model_ready_ms,
                self.first_embedding_done_ms,
            ),
            first_embedding_run_ms: duration_between(
                self.first_embedding_started_ms,
                self.first_embedding_done_ms,
            ),
            total_import_embedding_drain_ms: duration_between(
                self.first_embedding_queued_ms,
                self.all_embeddings_done_ms,
            ),
            model_unload_ms: duration_between(
                self.semantic_model_unload_requested_ms,
                self.semantic_model_unloaded_ms,
            ),
            model_load_timing: self.model_load_timing,
        })
    }
}

pub(super) fn duration_between(start_ms: Option<f64>, end_ms: Option<f64>) -> Option<f64> {
    match (start_ms, end_ms) {
        (Some(start_ms), Some(end_ms)) => Some((end_ms - start_ms).max(0.0)),
        _ => None,
    }
}

impl SnappyRefApp {
    pub(super) fn drive_perf_scenario(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) -> bool {
        if self.perf_scenario.is_none() {
            return false;
        }
        if self
            .perf_scenario
            .as_ref()
            .is_some_and(|scenario| !scenario.queued)
        {
            self.enqueue_perf_scenario_imports(ctx, screen_rect);
        }
        // trickle-import drips its remaining entries one per
        // `trickle_interval` so the **Embedding Helper Process** sees
        // gaps between successive imports.
        self.dispatch_perf_trickle_imports(ctx, screen_rect);
        self.update_perf_scenario_embedding_progress();
        self.finish_perf_scenario_if_idle(ctx)
    }

    pub(super) fn enqueue_perf_scenario_imports(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let Some(mut scenario) = self.perf_scenario.take() else {
            return;
        };
        scenario.queued = true;
        // Observe the normal startup load queued by `new`; do not import or
        // change the restored viewport to make the measurement look faster.
        if scenario.name == "startup" {
            scenario.emit(
                "scenario_started",
                serde_json::json!({ "expected_imports": scenario.expected_imports }),
            );
            self.perf_scenario = Some(scenario);
            return;
        }
        let entries = match scenario.selected_entries() {
            Ok(entries) => entries,
            Err(error) => {
                scenario.emit("error", serde_json::json!({ "message": error }));
                self.perf_scenario = Some(scenario);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        scenario.expected_imports = entries.len();
        scenario.emit(
            "scenario_started",
            serde_json::json!({ "expected_imports": entries.len() }),
        );
        let is_trickle = scenario.name == "trickle-import";
        self.perf_scenario = Some(scenario);

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);

        let entries = match self
            .perf_scenario
            .as_ref()
            .and_then(|scenario| scenario.selected_entries().ok())
        {
            Some(entries) => entries,
            None => return,
        };

        if is_trickle {
            // Enqueue the first import immediately so the
            // **Embedding Helper Process** spins up under the same
            // conditions as `import-1`; the remaining entries trickle
            // in via `dispatch_perf_trickle_imports`.
            let mut iter = entries.into_iter();
            if let Some(first) = iter.next() {
                if let Err(error) = self.enqueue_perf_entry(&first, desired_world, viewport_world) {
                    self.emit_perf_error_and_close(ctx, error);
                    return;
                }
            }
            let remaining: Vec<_> = iter.collect();
            if let Some(scenario) = &mut self.perf_scenario {
                let now = Instant::now();
                scenario.trickle_next_at =
                    remaining.first().map(|_| now + scenario.trickle_interval);
                scenario.pending_trickle_imports = remaining;
            }
        } else {
            for entry in entries {
                if let Err(error) = self.enqueue_perf_entry(&entry, desired_world, viewport_world) {
                    self.emit_perf_error_and_close(ctx, error);
                    return;
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    pub(super) fn enqueue_perf_entry(
        &mut self,
        entry: &PerfCorpusEntry,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
    ) -> Result<(), String> {
        let path = self
            .perf_scenario
            .as_ref()
            .map(|scenario| scenario.corpus_root.join(&entry.file))
            .unwrap_or_else(|| PathBuf::from(&entry.file));
        let dimensions = image::image_dimensions(&path).ok();
        let reservation = self.reserve_import(
            dimensions,
            desired_world,
            crate::data::ContentResolutionSource::ExternalPath {
                path: path.to_string_lossy().into_owned(),
            },
        )?;
        let request = crate::placement::content_resolution::import_request(
            ImportSource::Path(path.clone()),
            reservation,
            desired_world,
            viewport_world,
            self.import_request_config(),
            self.import_trace(),
        );
        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("perf scenario requires import worker")
            .enqueue(request)
        {
            self.remove_import_reservation(reservation);
            return Err(error);
        }
        if let Some(scenario) = &mut self.perf_scenario {
            let bytes = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
            scenario.asset_by_queue_id.insert(
                reservation.queue_id,
                PerfCorpusAsset {
                    file: entry.file.clone(),
                    format: entry.format.clone(),
                    animated: entry.animated,
                    bytes,
                },
            );
            scenario.emit(
                "import_queued",
                serde_json::json!({
                    "file": entry.file,
                    "format": entry.format,
                    "animated": entry.animated,
                    "role": entry.role,
                    "width": dimensions.map(|value| value.0),
                    "height": dimensions.map(|value| value.1),
                    "bytes": bytes,
                    "queue_id": reservation.queue_id,
                }),
            );
        }
        Ok(())
    }

    pub(super) fn dispatch_perf_trickle_imports(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let (next_at, interval) = match self.perf_scenario.as_ref() {
            Some(scenario) if !scenario.pending_trickle_imports.is_empty() => {
                (scenario.trickle_next_at, scenario.trickle_interval)
            }
            _ => return,
        };

        let Some(next_at) = next_at else {
            return;
        };
        if Instant::now() < next_at {
            // Make sure the egui main loop revisits us as soon as the
            // trickle window opens, even when nothing else is changing.
            ctx.request_repaint_after(next_at - Instant::now());
            return;
        }

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);

        // Pop one entry now, schedule the next interval if more remain.
        let entry = match self
            .perf_scenario
            .as_mut()
            .and_then(|scenario| scenario.pending_trickle_imports.first().cloned())
        {
            Some(entry) => entry,
            None => return,
        };

        if let Err(error) = self.enqueue_perf_entry(&entry, desired_world, viewport_world) {
            self.emit_perf_error_and_close(ctx, error);
            return;
        }

        if let Some(scenario) = &mut self.perf_scenario {
            scenario.pending_trickle_imports.remove(0);
            scenario.trickle_next_at = if scenario.pending_trickle_imports.is_empty() {
                None
            } else {
                Some(Instant::now() + interval)
            };
            if let Some(next) = scenario.trickle_next_at {
                ctx.request_repaint_after(next - Instant::now());
            } else {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
    }

    pub(super) fn emit_perf_error_and_close(&mut self, ctx: &egui::Context, error: String) {
        if let Some(scenario) = &mut self.perf_scenario {
            scenario.emit("error", serde_json::json!({ "message": error }));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub(super) fn update_perf_scenario_embedding_progress(&mut self) {
        let Some(scenario) = &mut self.perf_scenario else {
            return;
        };
        {
            for event in self.search.drain_perf_events() {
                match event {
                    crate::search::SemanticPerfEvent::ModelLoadStarted => {
                        scenario.emit("semantic_model_load_started", serde_json::json!({}));
                    }
                    crate::search::SemanticPerfEvent::ModelLoadTiming {
                        assets_resolved_ms,
                        tokenizer_loaded_ms,
                        text_session_loaded_ms,
                        vision_session_loaded_ms,
                        total_ms,
                    } => scenario.emit_model_load_timing(ModelLoadTimingSummary {
                        assets_resolved_ms,
                        tokenizer_loaded_ms,
                        text_session_loaded_ms,
                        vision_session_loaded_ms,
                        total_ms,
                    }),
                    crate::search::SemanticPerfEvent::ModelReady => {
                        scenario.emit("semantic_model_ready", serde_json::json!({}));
                    }
                    crate::search::SemanticPerfEvent::ImportEmbeddingStarted { asset_id } => {
                        let metadata = scenario.asset_by_asset_id.get(&asset_id).cloned();
                        let (format_value, source_file) = match &metadata {
                            Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                            None => (None, None),
                        };
                        scenario.emit(
                            "import_embedding_started",
                            serde_json::json!({
                                "asset_id": asset_id,
                                "format": format_value,
                                "file": source_file,
                            }),
                        );
                        if scenario
                            .embedding_phases
                            .first_embedding_started_ms
                            .is_none()
                        {
                            scenario.emit(
                                "first_import_embedding_started",
                                serde_json::json!({
                                    "asset_id": asset_id,
                                    "format": format_value,
                                    "file": source_file,
                                }),
                            );
                        }
                    }
                    crate::search::SemanticPerfEvent::ImportEmbeddingFinished { asset_id } => {
                        if !scenario.pending_embedding_asset_ids.remove(&asset_id) {
                            continue;
                        }
                        scenario.demand_embeddings_done += 1;
                        let metadata = scenario.asset_by_asset_id.get(&asset_id).cloned();
                        let (format_value, source_file) = match &metadata {
                            Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                            None => (None, None),
                        };
                        if !scenario.first_embedding_done_logged {
                            scenario.first_embedding_done_logged = true;
                            scenario.emit(
                                "first_import_embedding_done",
                                serde_json::json!({
                                    "asset_id": asset_id,
                                    "format": format_value,
                                    "file": source_file,
                                }),
                            );
                        }
                        scenario.emit(
                            "import_embedding_done",
                            serde_json::json!({
                                "done": scenario.demand_embeddings_done,
                                "asset_id": asset_id,
                                "format": format_value,
                                "file": source_file,
                            }),
                        );
                    }
                }
            }
            if scenario.demand_embeddings_queued > 0
                && scenario.loaded_imports >= scenario.expected_imports
                && scenario.pending_trickle_imports.is_empty()
                && scenario.demand_embeddings_done >= scenario.demand_embeddings_queued
                && !scenario.all_embeddings_done_logged
            {
                scenario.all_embeddings_done_logged = true;
                scenario.emit("all_import_embeddings_done", serde_json::json!({}));
            }
        }
    }

    pub(super) fn finish_perf_scenario_if_idle(&mut self, ctx: &egui::Context) -> bool {
        let Some(scenario) = &mut self.perf_scenario else {
            return false;
        };
        let idle_hold = std::env::var("SNAPPYREF_PERF_IDLE_HOLD_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(5));
        let worker_stats = self
            .import_worker
            .as_ref()
            .map(crate::jobs::ImagePrepWorker::stats)
            .unwrap_or_default();
        let embedding_idle = self.search.demand_embedding_pending_count() == 0
            && self.search.embedding_backfill_is_idle();
        let idle = scenario.queued
            && scenario.loaded_imports >= scenario.expected_imports
            && (if scenario.name == "startup" {
                scenario.expected_imports == 0 || scenario.visible_imports > 0
            } else {
                scenario.visible_imports >= scenario.expected_imports
            })
            && !worker_stats.has_work()
            && !self.canvas.has_pending_crop_work()
            && !self.canvas.has_pending_image_loads()
            && !self.residency.has_pending_backfills()
            && embedding_idle;

        if idle {
            let idle_since = *scenario.idle_since.get_or_insert_with(Instant::now);
            if idle_since.elapsed() >= Duration::from_millis(500)
                && scenario.settled_idle_at.is_none()
            {
                scenario.emit("settled_idle", serde_json::json!({}));
                scenario.write_summary(worker_stats, self.frame_perf.canvas.image_pass);
                // Background callbacks may arrive late. Give the sampler its
                // full hold after this marker, not after the earlier idle candidate.
                scenario.settled_idle_at = Some(Instant::now());
            }
            if let Some(settled_idle_at) = scenario.settled_idle_at {
                if settled_idle_at.elapsed() >= idle_hold {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return true;
                }
                let mut next_repaint_after = idle_hold.saturating_sub(settled_idle_at.elapsed());
                if let Some(unload_after) =
                    std::env::var("SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS")
                        .ok()
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis)
                {
                    if !scenario.model_unload_logged {
                        if idle_since.elapsed() >= unload_after {
                            if self.search.unload_model_for_perf() {
                                scenario.model_unload_logged = true;
                                scenario
                                    .emit("semantic_model_unload_requested", serde_json::json!({}));
                                scenario.emit("semantic_model_unloaded", serde_json::json!({}));
                            }
                        } else {
                            next_repaint_after =
                                next_repaint_after.min(unload_after - idle_since.elapsed());
                        }
                    }
                }
                ctx.request_repaint_after(next_repaint_after.max(Duration::from_millis(250)));
                return false;
            }
        } else {
            scenario.idle_since = None;
            if scenario.settled_idle_at.take().is_some() {
                scenario.emit("work_resumed", serde_json::json!({}));
                let _ = std::fs::remove_file(scenario.run_dir.join("summary.json"));
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        false
    }

    pub(super) fn record_perf_painted_frame(&mut self) {
        let Some(scenario) = &mut self.perf_scenario else {
            return;
        };
        if !scenario.first_frame_logged {
            scenario.first_frame_logged = true;
            scenario.emit("first_frame_painted", serde_json::json!({}));
        }
        if scenario.expected_imports == 0 || scenario.loaded_imports == 0 {
            return;
        }

        let image_pass = self.frame_perf.canvas.image_pass;
        let visible = image_pass
            .painted_images
            .min(scenario.loaded_imports)
            .min(scenario.expected_imports);
        if visible > scenario.visible_imports {
            scenario.visible_imports = visible;
        }
        if scenario.visible_imports == 0 {
            return;
        }

        let frame = serde_json::json!({
            "visible": scenario.visible_imports,
            "expected": scenario.expected_imports,
            "canvas_frame": {
                "visible_images": image_pass.visible_images,
                "painted_images": image_pass.painted_images,
                "visible_animated_images": image_pass.visible_animated_images,
                "active_animated_images": image_pass.active_animated_images,
                "fallback_images": image_pass.fallback_images,
                "draw_calls": image_pass.draw_calls,
                "texture_groups": image_pass.texture_groups,
                "build_time_ms": image_pass.build_time_ms,
            },
        });
        if !scenario.first_visible_logged {
            scenario.first_visible_logged = true;
            scenario.emit("first_placement_visible", frame.clone());
        }
        if scenario.visible_imports >= scenario.expected_imports && !scenario.all_visible_logged {
            scenario.all_visible_logged = true;
            scenario.emit("all_placements_visible", frame);
        }
    }

    /// True once all visible startup work queues have drained at least
    /// once — the same work-queue notion of Settled Idle that perf
    /// scenarios use, computed independently of the perf scenario.
    /// The update-discovery scheduler latches its own
    /// `settled_idle_at` on the first true return so subsequent dips
    /// back into busy state never re-block discovery.
    pub(super) fn app_settled_idle(&self) -> bool {
        if self.library.is_none() {
            // Storage failed at startup; treat as settled so the user
            // still gets Available Update controls if one exists.
            return true;
        }
        let worker_stats = self
            .import_worker
            .as_ref()
            .map(crate::jobs::ImagePrepWorker::stats)
            .unwrap_or_default();
        let embedding_idle = self.search.demand_embedding_pending_count() == 0
            && self.search.embedding_backfill_is_idle();
        !worker_stats.has_work()
            && !self.canvas.has_pending_crop_work()
            && !self.canvas.has_pending_image_loads()
            && !self.residency.has_pending_backfills()
            && embedding_idle
    }

    pub(super) fn log_loaded_timing(
        &mut self,
        origin: crate::jobs::ImportOrigin,
        placement: Option<&crate::data::Placement>,
        diagnostics: Option<&crate::jobs::ExistingLoadDiagnostics>,
        trace: Option<crate::jobs::ImagePrepTrace>,
    ) {
        let Some(perf_log) = &mut self.perf_log else {
            return;
        };

        let format_trace = |trace: crate::jobs::ImagePrepTrace| {
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
                let asset = placement.map(|placement| {
                    format!(
                        " placement_id={} asset_id={} file={}",
                        placement.state.id,
                        placement.require_loaded_asset().id,
                        placement.require_loaded_asset().filename
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

    pub(super) fn log_cropped_timing(
        &mut self,
        placement: &crate::data::Placement,
        ui_integration_ms: f64,
    ) {
        if self.perf_log.is_none() {
            return;
        }

        eprintln!(
            "[perf] crop_integrated placement_id={} asset_id={} file={} ui_ms={:.2}",
            placement.state.id,
            placement.require_loaded_asset().id,
            placement.require_loaded_asset().filename,
            ui_integration_ms
        );
    }
}
