use serde::{Deserialize, Serialize};

use super::CanvasBackground;

pub const DEFAULT_PLACEMENT_GAP: f32 = 0.0;
pub const DEFAULT_CANVAS_IMAGE_MAX_SIDE: u32 = 2000;
pub const DEFAULT_PLACEMENT_TARGET_AREA: f32 = 90_000.0;
pub const DEFAULT_ROTATION_SNAP_DEGREES: f32 = 15.0;
pub const DEFAULT_VIEWPORT_ZOOM: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanvasImageLod {
    Overview,
    Working,
    Detail,
}

impl CanvasImageLod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Working => "working",
            Self::Detail => "detail",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerMode {
    #[default]
    Mouse,
    Trackpad,
}

/// Whether an AI client may drive the app through `snappyref --mcp`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiControlAccess {
    #[default]
    Off,
    /// The user confirms in the app before an action Undo cannot reverse.
    Ask,
    Full,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewportState {
    pub camera_pan_x: f32,
    pub camera_pan_y: f32,
    pub camera_zoom: f32,
    pub canvas_background: CanvasBackground,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: DEFAULT_VIEWPORT_ZOOM,
            canvas_background: CanvasBackground::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(default)]
    pub outer_x: Option<f32>,
    #[serde(default)]
    pub outer_y: Option<f32>,
    #[serde(default)]
    pub inner_width: Option<f32>,
    #[serde(default)]
    pub inner_height: Option<f32>,
}

impl WindowState {
    pub fn is_restorable(&self) -> bool {
        self.outer_x.is_some()
            && self.outer_y.is_some()
            && self
                .inner_width
                .is_some_and(|width| width.is_finite() && width >= 320.0)
            && self
                .inner_height
                .is_some_and(|height| height.is_finite() && height >= 240.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSettings {
    #[serde(default = "default_placement_gap")]
    pub placement_gap: f32,
    #[serde(
        default = "default_placement_target_area",
        rename = "import_target_canvas_area"
    )]
    pub placement_target_area: f32,
    #[serde(default = "default_canvas_image_max_side")]
    pub canvas_image_max_side: u32,
    #[serde(default)]
    pub semantic_search_first_launch_prompt_seen: bool,
    #[serde(default = "default_rotation_snap_degrees")]
    pub rotation_snap_degrees: f32,
    #[serde(default)]
    pub pointer_mode: PointerMode,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default)]
    pub ai_control: AiControlAccess,
    /// Unix-epoch milliseconds of the most recent completed update
    /// discovery check. `None` means the app has not yet performed a
    /// check on this machine; About / Updates renders that as "never".
    #[serde(default)]
    pub last_update_check_at: Option<u64>,
}

fn default_placement_gap() -> f32 {
    DEFAULT_PLACEMENT_GAP
}

fn default_placement_target_area() -> f32 {
    DEFAULT_PLACEMENT_TARGET_AREA
}

fn default_canvas_image_max_side() -> u32 {
    DEFAULT_CANVAS_IMAGE_MAX_SIDE
}

fn default_rotation_snap_degrees() -> f32 {
    DEFAULT_ROTATION_SNAP_DEGREES
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            placement_gap: DEFAULT_PLACEMENT_GAP,
            placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
            canvas_image_max_side: DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            semantic_search_first_launch_prompt_seen: false,
            rotation_snap_degrees: DEFAULT_ROTATION_SNAP_DEGREES,
            pointer_mode: PointerMode::default(),
            always_on_top: false,
            ai_control: AiControlAccess::default(),
            last_update_check_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceSettings;

    #[test]
    fn removed_sizing_preference_preserves_supported_settings() {
        let settings: WorkspaceSettings = serde_json::from_str(
            r#"{"auto_image_scaling":false,"placement_gap":37.0,"always_on_top":true}"#,
        )
        .unwrap();
        assert_eq!(settings.placement_gap, 37.0);
        assert!(settings.always_on_top);
        assert!(
            !serde_json::to_string(&settings)
                .unwrap()
                .contains("auto_image_scaling")
        );
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserSession {
    pub active_board_id: i64,
    pub viewport: ViewportState,
    pub window: WindowState,
    pub settings: WorkspaceSettings,
    pub sidebar_open: bool,
    pub sidebar_tab: String,
    pub recent_searches: Vec<String>,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            active_board_id: 1,
            viewport: ViewportState::default(),
            window: WindowState::default(),
            settings: WorkspaceSettings::default(),
            sidebar_open: false,
            sidebar_tab: "settings".to_owned(),
            recent_searches: Vec::new(),
        }
    }
}
