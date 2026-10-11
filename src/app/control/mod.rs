//! Runs the tools an AI client calls through `snappyref --mcp`. Each call
//! runs on the UI thread through the owners a user gesture reaches, so the
//! board, its history and its persistence see one kind of edit.

mod describe;
mod edits;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::SnappyRefApp;
use super::shortcuts::PendingLibraryRemove;
use crate::control::{ControlReply, ControlServer, Responder};
use crate::data::{AiControlAccess, CanvasBackground, PointerMode};
use crate::search::semantic::{LocalModelStoreStatus, SearchQueryResult};

const NO_ANSWER: &str = "The user did not answer the confirmation in SnappyRef.";
const OTHER_DIALOG: &str =
    "SnappyRef is showing a dialog. Try again once the user has answered it.";
/// The long side of a screenshot an AI client reads without downscaling it again.
const SCREENSHOT_MAX_SIDE: u32 = 1568;

#[derive(Default)]
pub(super) struct AppControl {
    library_root: Option<PathBuf>,
    server: Option<ControlServer>,
    waiting: Vec<Waiting>,
    screenshots: u64,
}

/// A call that answers once something outside this frame finishes.
struct Waiting {
    responder: Responder,
    deadline: Instant,
    on: WaitingOn,
}

pub(super) enum WaitingOn {
    Screenshot(u64),
    /// Placements an import reserved, and the Board they load onto.
    Images {
        board: i64,
        ids: Vec<i64>,
    },
    Search {
        request_id: u64,
        query: Option<String>,
        similar_to: Option<i64>,
        limit: usize,
    },
    /// The job number of a background `.pur` import or export.
    PureRef(u64),
    Confirm(Confirm),
}

/// What the user answers in the app while AI control is set to Ask.
pub(super) enum Confirm {
    LibraryRemove(Vec<i64>),
    SearchModelRemove,
    SearchModelInstall,
}

pub(super) enum Step {
    Done(Value),
    Wait(Duration, WaitingOn),
}

fn arguments<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments).map_err(|error| format!("{error}."))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArguments {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionalBoard {
    board: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardArgument {
    board: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameBoard {
    board: i64,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateBoard {
    name: Option<String>,
    pureref: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddImages {
    sources: Vec<String>,
    x: Option<f32>,
    y: Option<f32>,
    group: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchLibrary {
    query: Option<String>,
    similar_to: Option<i64>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetView {
    fit: Option<Fit>,
    x: Option<f32>,
    y: Option<f32>,
    zoom: Option<f32>,
    background: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged, expecting = "true or a list of ids")]
enum Fit {
    Board(bool),
    Items(Vec<i64>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Select {
    ids: Vec<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportBoard {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSettings {
    placement_gap: Option<f32>,
    rotation_snap: Option<f32>,
    pointer: Option<PointerMode>,
    always_on_top: Option<bool>,
    sidebar_open: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveFromLibrary {
    assets: Vec<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchModel {
    action: SearchModelAction,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum SearchModelAction {
    Install,
    Remove,
}

fn absolute_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{} is not an absolute path.", path.display()))
    }
}

impl SnappyRefApp {
    /// Publishes or withdraws the listener to match the setting.
    pub(super) fn sync_control_server(&mut self, ctx: &egui::Context) {
        let Some(root) = &self.control.library_root else {
            return;
        };
        if self.settings.ai_control == AiControlAccess::Off {
            self.control.server = None;
            for waiting in self.control.waiting.drain(..) {
                if let WaitingOn::Search { request_id, .. } = waiting.on {
                    self.search.forget_direct_search(request_id);
                }
            }
            crate::control::withdraw(root);
        } else if self.control.server.is_none() {
            match ControlServer::start(root, ctx.clone()) {
                Ok(server) => self.control.server = Some(server),
                Err(error) => self.status = error,
            }
        }
    }

    /// The entry an AI client's MCP settings take to start `--mcp` against
    /// this app's Library.
    pub(super) fn copy_mcp_config(&mut self, ctx: &egui::Context) {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.status = format!("Failed to locate SnappyRef: {error}");
                return;
            }
        };
        let mut server = json!({"command": exe.to_string_lossy(), "args": ["--mcp"]});
        // A Library outside the default place has to reach `--mcp` too.
        let roots = [
            crate::local_data_location::SNAPPYREF_LIBRARY_ROOT_ENV,
            crate::local_data_location::SNAPPYREF_DATA_ROOT_ENV,
        ];
        for name in roots {
            if let Some(root) = std::env::var_os(name) {
                server["env"][name] = json!(root.to_string_lossy());
            }
        }
        let config = json!({"mcpServers": {"snappyref": server}});
        ctx.copy_text(serde_json::to_string_pretty(&config).unwrap_or_default());
        self.status = "Copied. Paste it into your AI client's MCP settings.".to_owned();
    }

    pub(super) fn start_control(&mut self, ctx: &egui::Context, library_root: PathBuf) {
        self.control.library_root = Some(library_root);
        self.sync_control_server(ctx);
    }

    pub(super) fn drive_control(&mut self, ctx: &egui::Context) {
        while let Some(request) = self
            .control
            .server
            .as_ref()
            .and_then(ControlServer::try_recv)
        {
            match self.control_call(ctx, &request.tool, request.arguments) {
                Ok(Step::Done(value)) => {
                    request.responder.send(ControlReply::from_result(Ok(value)))
                }
                Ok(Step::Wait(timeout, on)) => self.control.waiting.push(Waiting {
                    responder: request.responder,
                    deadline: Instant::now() + timeout,
                    on,
                }),
                Err(error) => request
                    .responder
                    .send(ControlReply::from_result(Err(error))),
            }
        }
        for waiting in std::mem::take(&mut self.control.waiting) {
            if let Some(waiting) = self.poll_waiting(ctx, waiting) {
                self.control.waiting.push(waiting);
            }
        }
        if !self.control.waiting.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    pub(super) fn control_call(
        &mut self,
        ctx: &egui::Context,
        tool: &str,
        input: Value,
    ) -> Result<Step, String> {
        let done = |result: Result<Value, String>| result.map(Step::Done);
        match tool {
            "get_board" => done(self.control_get_board(ctx, input)),
            "list_boards" => {
                arguments::<NoArguments>(input)?;
                let boards = self
                    .board_summaries()
                    .iter()
                    .map(|summary| {
                        json!({
                            "id": summary.board.id,
                            "name": summary.board.name,
                            "images": summary.placement_count,
                        })
                    })
                    .collect::<Vec<_>>();
                done(Ok(json!({"open": self.active_board_id, "boards": boards})))
            }
            "edit_board" => done(self.control_edit_board(ctx, input)),
            "add_images" => self.control_add_images(ctx, input),
            "search_library" => self.control_search_library(input),
            "create_board" => self.control_create_board(ctx, input),
            "open_board" => {
                let BoardArgument { board } = arguments(input)?;
                self.known_board(board)?;
                self.board_lifecycle().activate(board)?;
                done(Ok(self.open_board_json()))
            }
            "rename_board" => {
                let RenameBoard { board, name } = arguments(input)?;
                self.known_board(board)?;
                if name.trim().is_empty() {
                    return Err("\"name\" cannot be empty.".to_owned());
                }
                self.board_lifecycle().rename(board, &name)?;
                done(Ok(json!({"board": {"id": board, "name": name.trim()}})))
            }
            "delete_board" => {
                let BoardArgument { board } = arguments(input)?;
                self.known_board(board)?;
                self.board_lifecycle().delete(ctx, board)?;
                done(Ok(json!({"deleted": board, "open": self.active_board_id})))
            }
            "undo" | "redo" => {
                arguments::<NoArguments>(input)?;
                // An edit the app has not recorded yet is the newest Undo step.
                self.record_pending_document_changes_history("Edit board");
                let replayed = if tool == "undo" {
                    self.board_lifecycle().undo_document_history(ctx)?
                } else {
                    self.board_lifecycle().redo_document_history(ctx)?
                };
                let status = replayed.ok_or_else(|| format!("Nothing to {tool}."))?;
                done(Ok(json!({"status": status, "open": self.active_board_id})))
            }
            "set_view" => done(self.control_set_view(ctx, input)),
            "select" => {
                let Select { ids } = arguments(input)?;
                if let Some(id) = ids.iter().find(|id| !self.on_open_board(**id)) {
                    return Err(format!("No item or group has id {id}."));
                }
                self.canvas.select_ids(&ids);
                done(Ok(json!({"selection": ids})))
            }
            "screenshot" => {
                arguments::<NoArguments>(input)?;
                self.control.screenshots += 1;
                let request = self.control.screenshots;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    request,
                )));
                Ok(Step::Wait(
                    Duration::from_secs(5),
                    WaitingOn::Screenshot(request),
                ))
            }
            "export_board" => {
                let ExportBoard { path } = arguments(input)?;
                let path = absolute_path(&path)?;
                if !crate::pureref_exchange::is_pureref_board_path(&path) {
                    return Err(format!("{} does not end in .pur.", path.display()));
                }
                let plan = crate::pureref_exchange::PureRefExportPlan::from_canvas(&self.canvas);
                if plan.is_empty() {
                    return Err(plan.empty_status());
                }
                // The user's own export asks before it replaces a file.
                let job = self.pureref_exchange.start_export(
                    ctx,
                    plan,
                    path,
                    crate::pureref_exchange::ExistingFile::Keep,
                );
                Ok(Step::Wait(
                    Duration::from_secs(120),
                    WaitingOn::PureRef(job),
                ))
            }
            "get_settings" => {
                arguments::<NoArguments>(input)?;
                done(Ok(self.settings_json()))
            }
            "set_settings" => {
                let SetSettings {
                    placement_gap,
                    rotation_snap,
                    pointer,
                    always_on_top,
                    sidebar_open,
                } = arguments(input)?;
                let settings = &mut self.settings;
                if let Some(gap) = placement_gap {
                    settings.placement_gap = gap.clamp(0.0, 128.0);
                }
                if let Some(snap) = rotation_snap {
                    settings.rotation_snap_degrees = snap.clamp(0.0, 180.0);
                }
                settings.pointer_mode = pointer.unwrap_or(settings.pointer_mode);
                self.sidebar_open = sidebar_open.unwrap_or(self.sidebar_open);
                self.set_always_on_top(ctx, always_on_top.unwrap_or(self.settings.always_on_top));
                self.save_user_session();
                done(Ok(self.settings_json()))
            }
            "remove_from_library" => self.control_remove_from_library(ctx, input),
            "search_model" => self.control_search_model(ctx, input),
            _ => Err(format!("SnappyRef has no tool named \"{tool}\".")),
        }
    }

    fn known_board(&self, board: i64) -> Result<(), String> {
        if self
            .board_summaries()
            .iter()
            .any(|summary| summary.board.id == board)
        {
            Ok(())
        } else {
            Err(format!(
                "No board has id {board}. list_boards shows the boards."
            ))
        }
    }

    fn on_open_board(&self, id: i64) -> bool {
        describe::item(&self.canvas, id).is_some()
            || self.canvas.groups().iter().any(|group| group.id == id)
    }

    fn open_board_json(&self) -> Value {
        let name = self
            .board_summaries()
            .iter()
            .find(|summary| summary.board.id == self.active_board_id)
            .map(|summary| summary.board.name.as_str());
        json!({"board": {"id": self.active_board_id, "name": name}})
    }

    fn view_json(&self, ctx: &egui::Context) -> Value {
        let visible = self.import_viewport_world_rect(ctx.viewport_rect());
        let zoom = f64::from(self.canvas.viewport_state().camera_zoom);
        json!({
            "x": describe::round(visible.min.x),
            "y": describe::round(visible.min.y),
            "width": describe::round(visible.width()),
            "height": describe::round(visible.height()),
            "zoom": (zoom * 1000.0).round() / 1000.0,
        })
    }

    fn settings_json(&self) -> Value {
        let megabytes =
            crate::search::semantic::default_semantic_search_asset_descriptor().megabytes();
        let model = match self.search.local_model_store_status() {
            LocalModelStoreStatus::Installed => json!({"status": "installed"}),
            LocalModelStoreStatus::Missing => {
                json!({"status": "missing", "download_megabytes": megabytes})
            }
            LocalModelStoreStatus::Downloading { percent } => {
                json!({"status": "downloading", "percent": percent})
            }
            LocalModelStoreStatus::Failed(error) => json!({"status": "failed", "error": error}),
        };
        json!({
            "placement_gap": self.settings.placement_gap,
            "rotation_snap": self.settings.rotation_snap_degrees,
            "pointer": self.settings.pointer_mode,
            "always_on_top": self.settings.always_on_top,
            "sidebar_open": self.sidebar_open,
            "ai_control": self.settings.ai_control,
            "version": env!("CARGO_PKG_VERSION"),
            "search_model": model,
        })
    }

    fn control_get_board(&mut self, ctx: &egui::Context, input: Value) -> Result<Value, String> {
        let OptionalBoard { board } = arguments(input)?;
        let board = board.unwrap_or(self.active_board_id);
        if board != self.active_board_id {
            self.known_board(board)?;
            let library = self.library.as_ref().ok_or("Storage unavailable")?;
            let document = self.board_editing.accepted_board_document(library, board)?;
            let (items, groups) = describe::document(
                &document.placements,
                &document.annotations,
                &document.groups,
                &document.stack_order,
            );
            return Ok(json!({
                "board": {"id": board, "name": document.board.name},
                "items": items,
                "groups": groups,
            }));
        }
        let canvas = &self.canvas;
        let (items, groups) = describe::document(
            &canvas.committed_placements(),
            &canvas.annotations(),
            canvas.groups(),
            &canvas.canvas_stack_order(),
        );
        let mut selection = canvas.selected_placement_ids();
        selection.extend(canvas.selected_annotation_ids());
        selection.extend(canvas.selected_group_ids());
        let mut described = self.open_board_json();
        described["items"] = json!(items);
        described["groups"] = json!(groups);
        described["selection"] = json!(selection);
        described["view"] = self.view_json(ctx);
        described["background"] = json!(canvas.background().as_str());
        Ok(described)
    }

    fn control_add_images(&mut self, ctx: &egui::Context, input: Value) -> Result<Step, String> {
        let AddImages {
            sources,
            x,
            y,
            group,
        } = arguments(input)?;
        if sources.is_empty() {
            return Err("\"sources\" is empty.".to_owned());
        }
        let sources = sources
            .into_iter()
            .map(|source| {
                if source.starts_with("http://") || source.starts_with("https://") {
                    return Ok(crate::image_preparation::ImportSource::Url(source));
                }
                let path = absolute_path(&source)?;
                if !path.is_file() {
                    return Err(format!("{} is not a file.", path.display()));
                }
                Ok(crate::image_preparation::ImportSource::Path(path))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if let Some(group) = group
            && !self.canvas.groups().iter().any(|found| found.id == group)
        {
            return Err(format!("No group has id {group}."));
        }
        let screen_rect = ctx.viewport_rect();
        let center = match (x, y) {
            (Some(x), Some(y)) => egui::pos2(x, y),
            (None, None) => self.preferred_import_world_with_pointer(screen_rect, None),
            _ => return Err("Give x and y together, or neither.".to_owned()),
        };
        let before = self.canvas.all_placement_ids();
        // -1 appends to the Group.
        self.enqueue_sources_at(sources, screen_rect, center, group.map(|group| (group, -1)));
        let mut added = self.canvas.all_placement_ids();
        added.retain(|id| !before.contains(id));
        if added.is_empty() {
            return Err(self.status.clone());
        }
        Ok(Step::Wait(
            Duration::from_secs(60),
            WaitingOn::Images {
                board: self.active_board_id,
                ids: added,
            },
        ))
    }

    fn control_search_library(&mut self, input: Value) -> Result<Step, String> {
        let SearchLibrary {
            query,
            similar_to,
            limit,
        } = arguments(input)?;
        let query = query.filter(|query| !query.trim().is_empty());
        if query.is_none() && similar_to.is_none() {
            return Err("Give \"query\", \"similar_to\", or both.".to_owned());
        }
        match self.search.local_model_store_status() {
            LocalModelStoreStatus::Installed => {}
            LocalModelStoreStatus::Downloading { percent } => {
                return Err(format!(
                    "The search model is downloading, {percent}% done. Try again when \
                     get_settings reports it installed."
                ));
            }
            LocalModelStoreStatus::Missing | LocalModelStoreStatus::Failed(_) => {
                return Err(
                    "The search model is not installed. search_model installs it.".to_owned(),
                );
            }
        }
        let limit = limit.unwrap_or(20).clamp(1, 100);
        let request_id = self
            .search
            .start_direct_search(query.as_deref(), similar_to, limit)?;
        Ok(Step::Wait(
            Duration::from_secs(30),
            WaitingOn::Search {
                request_id,
                query,
                similar_to,
                limit,
            },
        ))
    }

    fn control_create_board(&mut self, ctx: &egui::Context, input: Value) -> Result<Step, String> {
        let CreateBoard { name, pureref } = arguments(input)?;
        match (name, pureref) {
            (Some(_), Some(_)) => {
                Err("A PureRef import names the board after its file. Give \"name\" or \"pureref\", not both.".to_owned())
            }
            (None, Some(path)) => {
                let path = absolute_path(&path)?;
                if !path.is_file() {
                    return Err(format!("{} is not a file.", path.display()));
                }
                let job = self
                    .start_pureref_import(ctx, path)
                    .ok_or_else(|| self.status.clone())?;
                Ok(Step::Wait(
                    Duration::from_secs(120),
                    WaitingOn::PureRef(job),
                ))
            }
            (Some(name), None) => {
                if name.trim().is_empty() {
                    return Err("\"name\" cannot be empty.".to_owned());
                }
                let library = self.library.as_ref().ok_or("Storage unavailable")?;
                let board = library.new_board(name.trim().to_owned());
                self.board_lifecycle()
                    .create_from_document(crate::data::BoardDocumentSnapshot {
                        board,
                        placements: Vec::new(),
                        annotations: Vec::new(),
                        groups: Vec::new(),
                        stack_order: Vec::new(),
                        viewport: Default::default(),
                    })?;
                Ok(Step::Done(self.open_board_json()))
            }
            (None, None) => {
                self.board_lifecycle().create()?;
                Ok(Step::Done(self.open_board_json()))
            }
        }
    }

    fn control_set_view(&mut self, ctx: &egui::Context, input: Value) -> Result<Value, String> {
        let SetView {
            fit,
            x,
            y,
            zoom,
            background,
        } = arguments(input)?;
        let background = background
            .map(|background| {
                CanvasBackground::from_str(&background)
                    .ok_or_else(|| format!("\"{background}\" is not one of none, grid, dots."))
            })
            .transpose()?;
        let screen = ctx.viewport_rect();
        let visible = self.visible_canvas_screen_rect(screen);
        let mut viewport = self.canvas.viewport_state();
        let ids = match fit {
            Some(Fit::Items(ids)) => Some(ids),
            Some(Fit::Board(true)) => Some(
                self.canvas
                    .canvas_stack_order()
                    .into_iter()
                    .map(|item| match item {
                        crate::data::CanvasStackItem::Placement(id)
                        | crate::data::CanvasStackItem::Annotation(id) => id,
                    })
                    .collect(),
            ),
            Some(Fit::Board(false)) | None => None,
        };
        if let Some(ids) = ids {
            if x.or(y).or(zoom).is_some() {
                return Err("Give \"fit\", or x, y and zoom, not both.".to_owned());
            }
            let mut bounds: Option<egui::Rect> = None;
            for id in ids {
                let rect = self
                    .item_bounds(id)
                    .ok_or_else(|| format!("No item or group has id {id}."))?;
                bounds = Some(bounds.map_or(rect, |bounds| bounds.union(rect)));
            }
            let bounds = bounds.ok_or("The board is empty, so there is nothing to fit.")?;
            let (pan, zoom) = crate::canvas::focus_framing(screen, visible, bounds);
            (viewport.camera_pan_x, viewport.camera_pan_y) = (pan.x, pan.y);
            viewport.camera_zoom = zoom;
        } else if x.or(y).or(zoom).is_some() {
            if zoom.is_some_and(|zoom| zoom <= 0.0) {
                return Err("\"zoom\" must be above 0.".to_owned());
            }
            let center = self.canvas.screen_to_world(visible.center(), screen);
            // The canvas clamps zoom, and the pan below depends on the zoom it kept.
            viewport.camera_zoom = zoom.unwrap_or(viewport.camera_zoom);
            self.canvas.apply_viewport_state(&viewport);
            viewport = self.canvas.viewport_state();
            let zoom = viewport.camera_zoom;
            // A missing axis keeps what the visible area centers on.
            let corner = egui::pos2(
                x.unwrap_or(center.x - visible.width() / (2.0 * zoom)),
                y.unwrap_or(center.y - visible.height() / (2.0 * zoom)),
            );
            let pan = visible.min - screen.center() - corner.to_vec2() * zoom;
            (viewport.camera_pan_x, viewport.camera_pan_y) = (pan.x, pan.y);
        }
        self.canvas.apply_viewport_state(&viewport);
        if let Some(background) = background {
            self.canvas.set_background(background);
        }
        Ok(json!({
            "view": self.view_json(ctx),
            "background": self.canvas.background().as_str(),
        }))
    }

    fn item_bounds(&self, id: i64) -> Option<egui::Rect> {
        if let Some(group) = self.canvas.groups().iter().find(|group| group.id == id) {
            return Some(group.container.visual_bounds());
        }
        if let Some(geometry) = self.canvas.rect_item_geometry(id) {
            return Some(geometry.visual_bounds());
        }
        let annotation = self.canvas.annotation_for_id(id)?;
        Some(egui::Rect::from_two_pos(
            annotation.start(),
            annotation.end(),
        ))
    }

    fn control_remove_from_library(
        &mut self,
        ctx: &egui::Context,
        input: Value,
    ) -> Result<Step, String> {
        let RemoveFromLibrary { assets } = arguments(input)?;
        if assets.is_empty() {
            return Err("\"assets\" is empty.".to_owned());
        }
        let library = self.library.as_ref().ok_or("Storage unavailable")?;
        let found = library.load_assets(&assets)?;
        if let Some(asset) = assets.iter().find(|asset| !found.contains_key(asset)) {
            return Err(format!("No library asset has id {asset}."));
        }
        self.confirm(ctx, Confirm::LibraryRemove(assets))
    }

    fn control_search_model(&mut self, ctx: &egui::Context, input: Value) -> Result<Step, String> {
        let SearchModel { action } = arguments(input)?;
        let status = self.search.local_model_store_status();
        match action {
            SearchModelAction::Install => match status {
                LocalModelStoreStatus::Missing | LocalModelStoreStatus::Failed(_) => {
                    return self.confirm(ctx, Confirm::SearchModelInstall);
                }
                LocalModelStoreStatus::Installed | LocalModelStoreStatus::Downloading { .. } => {}
            },
            SearchModelAction::Remove => match status {
                LocalModelStoreStatus::Missing => {
                    return Err("The search model is not installed.".to_owned());
                }
                LocalModelStoreStatus::Downloading { .. }
                    if self.settings.ai_control == AiControlAccess::Ask =>
                {
                    return Err(
                        "The search model is downloading. The user can cancel it in Settings."
                            .to_owned(),
                    );
                }
                LocalModelStoreStatus::Downloading { .. } => {
                    self.search.cancel_semantic_search_install(&mut self.status);
                }
                LocalModelStoreStatus::Installed | LocalModelStoreStatus::Failed(_) => {
                    return self.confirm(ctx, Confirm::SearchModelRemove);
                }
            },
        }
        Ok(Step::Done(self.settings_json()["search_model"].clone()))
    }

    /// Full access acts now. Ask raises the dialog the user would get from
    /// the app itself and waits for their answer.
    fn confirm(&mut self, ctx: &egui::Context, confirm: Confirm) -> Result<Step, String> {
        let ask = self.settings.ai_control == AiControlAccess::Ask;
        if ask && self.modal_open() {
            return Err(OTHER_DIALOG.to_owned());
        }
        match &confirm {
            Confirm::LibraryRemove(assets) if ask => {
                self.pending_library_remove = Some(PendingLibraryRemove {
                    asset_ids: assets.clone(),
                });
            }
            Confirm::LibraryRemove(assets) => self.remove_library_assets(assets),
            Confirm::SearchModelRemove if ask => self.pending_semantic_search_remove = true,
            Confirm::SearchModelRemove => self
                .search
                .remove_semantic_search_assets_from_settings(&mut self.status),
            // The first-launch prompt asks this same question.
            Confirm::SearchModelInstall if ask => {
                self.settings.semantic_search_first_launch_prompt_seen = false;
            }
            Confirm::SearchModelInstall => {
                self.search.start_semantic_search_install(&mut self.status);
            }
        }
        if ask && !self.confirm_open(&confirm) {
            self.dismiss_confirm(&confirm);
            return Err("SnappyRef cannot ask the user yet. Try again in a moment.".to_owned());
        }
        if ask {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
            return Ok(Step::Wait(
                Duration::from_secs(60),
                WaitingOn::Confirm(confirm),
            ));
        }
        self.confirmed(&confirm).map(Step::Done)
    }

    /// Says who is asking when the open dialog answers an AI client's call.
    /// Ask raises one dialog at a time, so a waiting call owns the open one.
    pub(super) fn ai_request_note(&self, detail: &str) -> String {
        let asked = |waiting: &Waiting| matches!(waiting.on, WaitingOn::Confirm(_));
        if self.control.waiting.iter().any(asked) {
            format!("An AI client asked for this. {detail}")
        } else {
            detail.to_owned()
        }
    }

    fn confirm_open(&self, confirm: &Confirm) -> bool {
        match confirm {
            Confirm::LibraryRemove(_) => self.pending_library_remove.is_some(),
            Confirm::SearchModelRemove => self.pending_semantic_search_remove,
            Confirm::SearchModelInstall => self.should_show_semantic_search_first_launch_prompt(),
        }
    }

    /// Reads the outcome from app state once no dialog is open.
    fn confirmed(&self, confirm: &Confirm) -> Result<Value, String> {
        let status = self.search.local_model_store_status();
        let done = match confirm {
            Confirm::LibraryRemove(assets) => {
                let library = self.library.as_ref().ok_or("Storage unavailable")?;
                library.load_assets(assets)?.is_empty()
            }
            Confirm::SearchModelRemove => status == LocalModelStoreStatus::Missing,
            Confirm::SearchModelInstall => matches!(
                status,
                LocalModelStoreStatus::Installed | LocalModelStoreStatus::Downloading { .. }
            ),
        };
        if !done {
            return Err(match self.settings.ai_control {
                AiControlAccess::Ask => "The user declined in SnappyRef.".to_owned(),
                _ => self.status.clone(),
            });
        }
        Ok(match confirm {
            Confirm::LibraryRemove(assets) => json!({"removed": assets}),
            _ => self.settings_json()["search_model"].clone(),
        })
    }

    fn dismiss_confirm(&mut self, confirm: &Confirm) {
        match confirm {
            Confirm::LibraryRemove(_) => self.pending_library_remove = None,
            Confirm::SearchModelRemove => self.pending_semantic_search_remove = false,
            Confirm::SearchModelInstall => self.mark_semantic_search_first_launch_prompt_seen(),
        }
    }

    /// Answers the call and returns None, or hands it back to keep waiting.
    fn poll_waiting(&mut self, ctx: &egui::Context, mut waiting: Waiting) -> Option<Waiting> {
        let timed_out = Instant::now() >= waiting.deadline;
        let result = match &mut waiting.on {
            WaitingOn::Screenshot(request) => {
                let request = *request;
                let image = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot {
                            user_data, image, ..
                        } if user_data
                            .data
                            .as_ref()
                            .and_then(|data| data.downcast_ref::<u64>())
                            == Some(&request) =>
                        {
                            Some(image.clone())
                        }
                        _ => None,
                    })
                });
                match image {
                    Some(image) => {
                        // Encoding a window-sized PNG takes longer than a frame.
                        std::thread::spawn(move || {
                            waiting.responder.send(screenshot_reply(&image))
                        });
                        return None;
                    }
                    None if timed_out => Err(
                        "SnappyRef could not capture its window. It may be hidden or minimized."
                            .to_owned(),
                    ),
                    None => return Some(waiting),
                }
            }
            WaitingOn::Images { board, ids } => match self.added_images(*board, ids, timed_out) {
                Some(result) => result,
                None => return Some(waiting),
            },
            WaitingOn::Search {
                request_id,
                query,
                similar_to,
                limit,
            } => match self.search.take_direct_search_result(*request_id) {
                Some(SearchQueryResult::Completed { results, .. }) => {
                    let ids = results
                        .iter()
                        .map(|result| result.asset_id)
                        .collect::<Vec<_>>();
                    let files = self
                        .library
                        .as_ref()
                        .and_then(|library| library.load_assets(&ids).ok())
                        .unwrap_or_default();
                    let assets = results
                        .iter()
                        .map(|result| {
                            json!({
                                "asset": result.asset_id,
                                "width": result.asset_width,
                                "height": result.asset_height,
                                "file": files.get(&result.asset_id).map(|asset| &asset.filename),
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(json!({"assets": assets}))
                }
                Some(SearchQueryResult::Failed { error, .. }) => Err(error),
                // The model was still loading. Ask again until it answers.
                Some(SearchQueryResult::ModelPending { .. }) if !timed_out => {
                    match self
                        .search
                        .start_direct_search(query.as_deref(), *similar_to, *limit)
                    {
                        Ok(next) => {
                            *request_id = next;
                            return Some(waiting);
                        }
                        Err(error) => Err(error),
                    }
                }
                None if !timed_out => return Some(waiting),
                _ => {
                    self.search.forget_direct_search(*request_id);
                    Err("The search model is still loading. Try again in a moment.".to_owned())
                }
            },
            WaitingOn::PureRef(_) if timed_out => Err(
                "The PureRef file took too long. SnappyRef finishes it in the background."
                    .to_owned(),
            ),
            WaitingOn::PureRef(_) => return Some(waiting),
            WaitingOn::Confirm(confirm) => {
                if !self.confirm_open(confirm) {
                    self.confirmed(confirm)
                } else if timed_out {
                    self.dismiss_confirm(confirm);
                    Err(NO_ANSWER.to_owned())
                } else {
                    return Some(waiting);
                }
            }
        };
        waiting.responder.send(ControlReply::from_result(result));
        None
    }

    /// What `add_images` answers, or None while an image still loads. The
    /// images load onto the Board the call reserved them on, even after the
    /// user opens another.
    fn added_images(
        &self,
        board: i64,
        ids: &[i64],
        timed_out: bool,
    ) -> Option<Result<Value, String>> {
        let items = if board == self.active_board_id {
            ids.iter()
                .filter_map(|id| describe::item(&self.canvas, *id))
                .collect::<Vec<_>>()
        } else {
            match self.accepted_items(board, ids) {
                Ok(items) => items,
                Err(error) => return Some(Err(error)),
            }
        };
        if !timed_out && items.iter().any(|item| item["status"] == "loading") {
            return None;
        }
        if items.is_empty() {
            return Some(Err(format!("No image was added. {}", self.status)));
        }
        let mut added = json!({"items": items});
        if items.len() < ids.len() {
            added["failed"] = json!(ids.len() - items.len());
            added["status"] = json!(self.status);
        }
        if board != self.active_board_id {
            added["note"] = json!(format!(
                "The open board changed. These images are on board {board}."
            ));
        }
        Some(Ok(added))
    }

    /// Items of a Board that is not open, read from its accepted document.
    fn accepted_items(&self, board: i64, ids: &[i64]) -> Result<Vec<Value>, String> {
        let library = self.library.as_ref().ok_or("Storage unavailable")?;
        let document = self.board_editing.accepted_board_document(library, board)?;
        let (mut items, _) = describe::document(
            &document.placements,
            &document.annotations,
            &document.groups,
            &document.stack_order,
        );
        items.retain(|item| item["id"].as_i64().is_some_and(|id| ids.contains(&id)));
        Ok(items)
    }

    /// A PureRef import or export finished. The user's own imports and
    /// exports pass through here too and answer no call.
    pub(super) fn answer_pureref_control_call(
        &mut self,
        job: u64,
        outcome: Result<String, String>,
    ) {
        let Some(index) = self.control.waiting.iter().position(
            |waiting| matches!(waiting.on, WaitingOn::PureRef(running) if running == job),
        ) else {
            return;
        };
        let result = outcome.map(|status| {
            let mut described = self.open_board_json();
            described["status"] = json!(status);
            described
        });
        let waiting = self.control.waiting.remove(index);
        waiting.responder.send(ControlReply::from_result(result));
    }
}

fn screenshot_reply(image: &egui::ColorImage) -> ControlReply {
    let [width, height] = image.size.map(|side| side as u32);
    let encoded = image::RgbaImage::from_raw(width, height, image.as_raw().to_vec())
        .ok_or_else(|| "The window capture was malformed.".to_owned())
        .and_then(|mut capture| {
            let scale = SCREENSHOT_MAX_SIDE as f32 / width.max(height) as f32;
            if scale < 1.0 {
                capture = image::imageops::resize(
                    &capture,
                    ((width as f32 * scale).round() as u32).max(1),
                    ((height as f32 * scale).round() as u32).max(1),
                    image::imageops::FilterType::Triangle,
                );
            }
            let mut png = std::io::Cursor::new(Vec::new());
            capture
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(|error| format!("Failed to encode the screenshot: {error}"))?;
            Ok((capture.width(), capture.height(), png.into_inner()))
        });
    match encoded {
        Ok((width, height, png)) => ControlReply {
            result: Ok(json!({"width": width, "height": height})),
            png: Some(png),
        },
        Err(error) => ControlReply::from_result(Err(error)),
    }
}
