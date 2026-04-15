use std::path::Path;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::native_capture::NativeWindowCapture;
use crate::storage::Library;

#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use std::collections::VecDeque;
#[cfg(debug_assertions)]
use std::path::PathBuf;
#[cfg(debug_assertions)]
use serde::Serialize;
#[cfg(debug_assertions)]
use serde::Deserialize;
#[cfg(debug_assertions)]
use crate::jobs::LoadedCanvasItem;
#[cfg(debug_assertions)]
use std::time::Duration;

#[cfg(debug_assertions)]
const DEV_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub enum DevHarnessAction {
    None,
    CloseApp,
}

pub struct DevHarness {
    inner: Option<InnerDevHarness>,
}

#[cfg(not(debug_assertions))]
struct InnerDevHarness {
    overlay_visible: bool,
}

#[cfg(not(debug_assertions))]
impl InnerDevHarness {
    fn begin_frame(
        &mut self,
        _ctx: &egui::Context,
        _screen_rect: egui::Rect,
        _canvas: &mut CanvasState,
        _window_capture: Option<&NativeWindowCapture>,
        _library: Option<&Library>,
        _status: &mut String,
    ) -> DevHarnessAction {
        DevHarnessAction::None
    }

    fn show_overlay_ui(
        &self,
        _ctx: &egui::Context,
        _screen_rect: egui::Rect,
        _canvas: &CanvasState,
        _status: &str,
    ) {
    }

    fn has_pending_screenshot(&self) -> bool {
        false
    }
}

impl DevHarness {
    pub fn new(enabled: bool, library_root: &Path) -> Result<Self, String> {
        #[cfg(debug_assertions)]
        {
            let inner = if enabled {
                Some(InnerDevHarness::new(library_root)?)
            } else {
                None
            };
            return Ok(Self { inner });
        }

        #[cfg(not(debug_assertions))]
        {
            let _ = (enabled, library_root);
            Ok(Self { inner: None })
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub fn show_overlay(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.overlay_visible)
    }

    pub fn has_pending_screenshot(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.has_pending_screenshot())
    }

    pub fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
        window_capture: Option<&NativeWindowCapture>,
        library: Option<&Library>,
        status: &mut String,
    ) -> DevHarnessAction {
        let Some(inner) = &mut self.inner else {
            return DevHarnessAction::None;
        };

        inner.begin_frame(ctx, screen_rect, canvas, window_capture, library, status)
    }

    pub fn show_overlay_ui(
        &self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &CanvasState,
        status: &str,
    ) {
        let Some(inner) = &self.inner else {
            return;
        };

        if inner.overlay_visible {
            inner.show_overlay_ui(ctx, screen_rect, canvas, status);
        }
    }
}

#[cfg(debug_assertions)]
struct InnerDevHarness {
    workspace_root: PathBuf,
    dev_root: PathBuf,
    command_path: PathBuf,
    result_path: PathBuf,
    manifest_path: PathBuf,
    overlay_visible: bool,
    last_completed_command_id: Option<String>,
    pending_command: Option<PendingCommand>,
    pending_screenshot: Option<PathBuf>,
}

#[cfg(debug_assertions)]
impl InnerDevHarness {
    fn new(library_root: &Path) -> Result<Self, String> {
        let dev_root = library_root.join("dev");
        let workspace_root = library_root
            .parent()
            .unwrap_or(library_root)
            .to_path_buf();
        fs::create_dir_all(dev_root.join("out"))
            .map_err(|error| format!("failed to create dev harness dir: {error}"))?;

        let harness = Self {
            workspace_root,
            command_path: dev_root.join("command.json"),
            result_path: dev_root.join("last-result.json"),
            manifest_path: dev_root.join("manifest.json"),
            dev_root,
            overlay_visible: true,
            last_completed_command_id: None,
            pending_command: None,
            pending_screenshot: None,
        };
        harness.write_manifest()?;
        Ok(harness)
    }

    fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
        window_capture: Option<&NativeWindowCapture>,
        library: Option<&Library>,
        status: &mut String,
    ) -> DevHarnessAction {
        ctx.request_repaint_after(DEV_POLL_INTERVAL);

        if ctx.input_mut(|input| input.consume_shortcut(&overlay_shortcut())) {
            self.overlay_visible = !self.overlay_visible;
        }

        if let Err(error) = self.handle_screenshot_event(ctx) {
            *status = error.clone();
            self.fail_pending_command(&error);
        }

        if self.pending_command.is_none()
            && let Some(command) = self.load_next_command(status)
        {
            self.write_result(CommandResult::running(&command.id, 0, "command accepted"));
            self.pending_command = Some(PendingCommand::from(command));
        }

        let mut next_action = DevHarnessAction::None;

        if let Some(mut command) = self.pending_command.take() {
            loop {
                if self.pending_screenshot.is_some() {
                    self.pending_command = Some(command);
                    break;
                }

                let Some(action) = command.actions.pop_front() else {
                    let command_id = command.id.clone();
                    self.last_completed_command_id = Some(command_id.clone());
                    self.write_result(CommandResult::completed(
                        &command_id,
                        command.total_actions,
                        "command completed",
                    ));
                    break;
                };

                match self.apply_action(ctx, screen_rect, canvas, window_capture, library, &action) {
                    Ok(action_result) => {
                        let completed_actions = command.total_actions - command.actions.len();
                        self.write_result(CommandResult::running(
                            &command.id,
                            completed_actions,
                            &format!("processed {}", action.kind_name()),
                        ));
                        if matches!(action_result, DevHarnessAction::CloseApp) {
                            next_action = DevHarnessAction::CloseApp;
                        }
                        if self.pending_screenshot.is_some() {
                            self.pending_command = Some(command);
                            break;
                        }
                    }
                    Err(error) => {
                        *status = error.clone();
                        self.pending_command = Some(command);
                        self.fail_pending_command(&error);
                        break;
                    }
                }
            }
        }

        next_action
    }

    fn apply_action(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
        window_capture: Option<&NativeWindowCapture>,
        library: Option<&Library>,
        action: &DevAction,
    ) -> Result<DevHarnessAction, String> {
        match action {
            DevAction::LoadFixture { name } => {
                let fixture = build_fixture(name)?;
                canvas.replace_items(ctx, fixture);
                Ok(DevHarnessAction::None)
            }
            DevAction::ClearBoard => {
                canvas.clear();
                Ok(DevHarnessAction::None)
            }
            DevAction::SetCamera { pan_x, pan_y, zoom } => {
                canvas.set_camera(*pan_x, *pan_y, *zoom);
                Ok(DevHarnessAction::None)
            }
            DevAction::SelectItems { item_ids } => {
                canvas.select_items(item_ids);
                Ok(DevHarnessAction::None)
            }
            DevAction::MoveSelected { dx, dy } => {
                canvas.move_selected_by(*dx, *dy);
                Ok(DevHarnessAction::None)
            }
            DevAction::ImportFile { path, x, y } => {
                let library = library.ok_or_else(|| "library unavailable".to_owned())?;
                let desired_world = match (x, y) {
                    (Some(x), Some(y)) => egui::pos2(*x, *y),
                    (None, None) => canvas.viewport_center_world(screen_rect),
                    _ => {
                        return Err(
                            "import_file requires both x and y, or neither to use viewport center"
                                .to_owned(),
                        )
                    }
                };
                let source_path = self.resolve_import_source_path(path);
                let loaded = crate::jobs::import_path_sync(library, source_path, desired_world)?;
                canvas.add_loaded_item(ctx, loaded);
                Ok(DevHarnessAction::None)
            }
            DevAction::FocusItem { item_id, zoom } => {
                canvas.focus_item(*item_id, screen_rect, *zoom)?;
                Ok(DevHarnessAction::None)
            }
            DevAction::DeleteSelected => {
                let deleted_ids = canvas.delete_selected();
                if let Some(library) = library {
                    let persisted_ids = deleted_ids
                        .into_iter()
                        .filter(|item_id| *item_id > 0)
                        .collect::<Vec<_>>();
                    library.delete_board_items(&persisted_ids)?;
                }
                Ok(DevHarnessAction::None)
            }
            DevAction::SetItemPosition { item_id, x, y } => {
                canvas.set_item_position(*item_id, *x, *y)?;
                Ok(DevHarnessAction::None)
            }
            DevAction::SetItemScale { item_id, scale } => {
                canvas.set_item_scale(*item_id, *scale)?;
                Ok(DevHarnessAction::None)
            }
            DevAction::DumpState { path } => {
                let resolved_path = self.resolve_path(path);
                let snapshot = canvas.snapshot(screen_rect);
                write_json_file(&resolved_path, &snapshot)?;
                Ok(DevHarnessAction::None)
            }
            DevAction::Screenshot { path } => {
                let resolved_path = self.resolve_path(path);
                if let Some(window_capture) = window_capture {
                    window_capture.capture_png(&resolved_path)?;
                } else {
                    self.pending_screenshot = Some(resolved_path);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                    ctx.request_repaint();
                }
                Ok(DevHarnessAction::None)
            }
            DevAction::SetOverlay { visible } => {
                self.overlay_visible = *visible;
                Ok(DevHarnessAction::None)
            }
            DevAction::Quit => Ok(DevHarnessAction::CloseApp),
        }
    }

    fn handle_screenshot_event(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(path) = self.pending_screenshot.clone() else {
            return Ok(());
        };

        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });

        let Some(image) = screenshot else {
            return Ok(());
        };

        let pixels = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect::<Vec<_>>();

        write_parent_dir(&path)?;
        image::save_buffer_with_format(
            &path,
            &pixels,
            image.width() as u32,
            image.height() as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|error| format!("failed to write screenshot to {}: {error}", path.display()))?;

        self.pending_screenshot = None;
        Ok(())
    }

    fn load_next_command(&self, status: &mut String) -> Option<DevCommandFile> {
        let contents = match fs::read_to_string(&self.command_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => {
                *status = format!("Dev harness command read failed: {error}");
                return None;
            }
        };

        let command = match serde_json::from_str::<DevCommandFile>(&contents) {
            Ok(command) => command,
            Err(error) => {
                *status = format!("Dev harness command parse failed: {error}");
                return None;
            }
        };

        if self.last_completed_command_id.as_deref() == Some(command.id.as_str()) {
            return None;
        }

        Some(command)
    }

    fn show_overlay_ui(
        &self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &CanvasState,
        status: &str,
    ) {
        let snapshot = canvas.snapshot(screen_rect);
        let pending_id = self.pending_command.as_ref().map(|command| command.id.as_str());

        egui::Area::new(egui::Id::new("dev_harness_overlay"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(crate::theme::SHADE_80)
                    .stroke(egui::Stroke::new(1.0, crate::theme::SHADE_40))
                    .inner_margin(8)
                    .show(ui, |ui| {
                        ui.set_max_width(320.0);
                        ui.colored_label(crate::theme::SHADE_BASE, "Dev Harness");
                        ui.colored_label(crate::theme::SHADE_20, self.command_path.display().to_string());
                        ui.colored_label(crate::theme::SHADE_30, format!("result: {}", self.result_path.display()));
                        ui.add_space(6.0);
                        ui.label(format!("camera: ({:.1}, {:.1}) @ {:.3}", snapshot.camera_pan_x, snapshot.camera_pan_y, snapshot.camera_zoom));
                        ui.label(format!("items: {}", snapshot.items.len()));
                        ui.label(format!("selected: {:?}", snapshot.selected_item_ids));
                        if let Some(command_id) = pending_id {
                            ui.label(format!("pending: {command_id}"));
                        }
                        ui.label(format!("status: {status}"));
                        ui.label("toggle overlay: Cmd/Ctrl+Shift+D");
                    });
            });
    }

    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.dev_root.join(path)
        }
    }

    fn resolve_import_source_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }

        let workspace_path = self.workspace_root.join(path);
        if workspace_path.exists() {
            return workspace_path;
        }

        self.dev_root.join(path)
    }

    fn fail_pending_command(&mut self, error: &str) {
        if let Some(command) = self.pending_command.take() {
            self.write_result(CommandResult::failed(&command.id, error));
        }
        self.pending_screenshot = None;
    }

    fn write_manifest(&self) -> Result<(), String> {
        let manifest = DevManifest {
            command_path: self.command_path.clone(),
            result_path: self.result_path.clone(),
            dev_root: self.dev_root.clone(),
            workspace_root: self.workspace_root.clone(),
            supported_actions: vec![
                "load_fixture",
                "clear_board",
                "set_camera",
                "select_items",
                "move_selected",
                "import_file",
                "focus_item",
                "delete_selected",
                "set_item_position",
                "set_item_scale",
                "dump_state",
                "screenshot",
                "set_overlay",
                "quit",
            ],
        };
        write_json_file(&self.manifest_path, &manifest)
    }

    fn write_result(&self, result: CommandResult) {
        let _ = write_json_file(&self.result_path, &result);
    }

    fn has_pending_screenshot(&self) -> bool {
        self.pending_screenshot.is_some()
    }
}

#[cfg(debug_assertions)]
struct PendingCommand {
    id: String,
    total_actions: usize,
    actions: VecDeque<DevAction>,
}

#[cfg(debug_assertions)]
impl PendingCommand {
    fn from(command: DevCommandFile) -> Self {
        let total_actions = command.actions.len();
        Self {
            id: command.id,
            total_actions,
            actions: command.actions.into(),
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize, Serialize, Clone)]
struct DevCommandFile {
    id: String,
    actions: Vec<DevAction>,
}

#[cfg(debug_assertions)]
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DevAction {
    LoadFixture { name: String },
    ClearBoard,
    SetCamera { pan_x: f32, pan_y: f32, zoom: f32 },
    SelectItems { item_ids: Vec<i64> },
    MoveSelected { dx: f32, dy: f32 },
    ImportFile { path: PathBuf, x: Option<f32>, y: Option<f32> },
    FocusItem { item_id: i64, zoom: Option<f32> },
    DeleteSelected,
    SetItemPosition { item_id: i64, x: f32, y: f32 },
    SetItemScale { item_id: i64, scale: f32 },
    DumpState { path: PathBuf },
    Screenshot { path: PathBuf },
    SetOverlay { visible: bool },
    Quit,
}

#[cfg(debug_assertions)]
impl DevAction {
    fn kind_name(&self) -> &'static str {
        match self {
            DevAction::LoadFixture { .. } => "load_fixture",
            DevAction::ClearBoard => "clear_board",
            DevAction::SetCamera { .. } => "set_camera",
            DevAction::SelectItems { .. } => "select_items",
            DevAction::MoveSelected { .. } => "move_selected",
            DevAction::ImportFile { .. } => "import_file",
            DevAction::FocusItem { .. } => "focus_item",
            DevAction::DeleteSelected => "delete_selected",
            DevAction::SetItemPosition { .. } => "set_item_position",
            DevAction::SetItemScale { .. } => "set_item_scale",
            DevAction::DumpState { .. } => "dump_state",
            DevAction::Screenshot { .. } => "screenshot",
            DevAction::SetOverlay { .. } => "set_overlay",
            DevAction::Quit => "quit",
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Serialize)]
struct CommandResult {
    id: String,
    status: &'static str,
    message: String,
    completed_actions: usize,
}

#[cfg(debug_assertions)]
impl CommandResult {
    fn running(id: &str, completed_actions: usize, message: &str) -> Self {
        Self {
            id: id.to_owned(),
            status: "running",
            message: message.to_owned(),
            completed_actions,
        }
    }

    fn completed(id: &str, completed_actions: usize, message: &str) -> Self {
        Self {
            id: id.to_owned(),
            status: "completed",
            message: message.to_owned(),
            completed_actions,
        }
    }

    fn failed(id: &str, message: &str) -> Self {
        Self {
            id: id.to_owned(),
            status: "failed",
            message: message.to_owned(),
            completed_actions: 0,
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Serialize)]
struct DevManifest {
    command_path: PathBuf,
    result_path: PathBuf,
    dev_root: PathBuf,
    workspace_root: PathBuf,
    supported_actions: Vec<&'static str>,
}

#[cfg(debug_assertions)]
fn build_fixture(name: &str) -> Result<Vec<LoadedCanvasItem>, String> {
    match name {
        "basic" | "basic-board" => Ok(vec![
            fixture_item(-101, -201, "fixture-alpha.png", egui::pos2(-220.0, -100.0), [260, 180], [0xC4, 0x7E, 0x56]),
            fixture_item(-102, -202, "fixture-beta.png", egui::pos2(40.0, 20.0), [220, 160], [0x6D, 0x8B, 0x74]),
            fixture_item(-103, -203, "fixture-gamma.png", egui::pos2(250.0, -150.0), [180, 240], [0x7B, 0x88, 0xA0]),
        ]),
        other => Err(format!("unknown fixture '{other}'. Supported fixtures: basic")),
    }
}

#[cfg(debug_assertions)]
fn fixture_item(
    item_id: i64,
    asset_id: i64,
    filename: &str,
    position: egui::Pos2,
    size: [usize; 2],
    rgb: [u8; 3],
) -> LoadedCanvasItem {
    let rgba = fixture_rgba(size, rgb);
    LoadedCanvasItem {
        record: crate::data::BoardItemWithAsset {
            asset: crate::data::ImageAsset {
                id: asset_id,
                original_path: PathBuf::from(format!("dev://fixture/{filename}")),
                filename: filename.to_owned(),
                mime_type: "image/png".to_owned(),
                width: size[0] as u32,
                height: size[1] as u32,
                file_size: rgba.len() as u64,
                imported_at: 0,
            },
            item: crate::data::BoardItem {
                id: item_id,
                asset_id,
                x: position.x,
                y: position.y,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
            },
        },
        rgba,
    }
}

#[cfg(debug_assertions)]
fn fixture_rgba(size: [usize; 2], rgb: [u8; 3]) -> Vec<u8> {
    let mut rgba = vec![0_u8; size[0] * size[1] * 4];

    for y in 0..size[1] {
        for x in 0..size[0] {
            let index = (y * size[0] + x) * 4;
            let shade = if (x / 16 + y / 16) % 2 == 0 { 0 } else { 14 };
            rgba[index] = rgb[0].saturating_add(shade);
            rgba[index + 1] = rgb[1].saturating_add(shade);
            rgba[index + 2] = rgb[2].saturating_add(shade);
            rgba[index + 3] = 255;
        }
    }

    rgba
}

#[cfg(debug_assertions)]
fn overlay_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::D,
    )
}

#[cfg(debug_assertions)]
fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    write_parent_dir(path)?;
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("failed to serialize JSON for {}: {error}", path.display()))?;
    fs::write(path, json).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(debug_assertions)]
fn write_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    Ok(())
}
