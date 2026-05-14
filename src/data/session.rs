use serde::{Deserialize, Serialize};

use super::CanvasBackground;

pub const DEFAULT_IMPORT_GAP: f32 = 0.0;
pub const DEFAULT_CANVAS_IMAGE_MAX_SIDE: u32 = 2000;
pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = 90_000.0;
pub const DEFAULT_ROTATION_SNAP_DEGREES: f32 = 15.0;

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
            camera_zoom: 1.0,
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
    #[serde(default = "default_auto_image_scaling")]
    pub auto_image_scaling: bool,
    #[serde(default = "default_import_gap")]
    pub import_gap: f32,
    #[serde(default = "default_import_target_canvas_area")]
    pub import_target_canvas_area: f32,
    #[serde(default = "default_canvas_image_max_side")]
    pub canvas_image_max_side: u32,
    #[serde(default = "default_semantic_search_enabled")]
    pub semantic_search_enabled: bool,
    #[serde(default)]
    pub semantic_search_first_launch_prompt_seen: bool,
    #[serde(default = "default_rotation_snap_degrees")]
    pub rotation_snap_degrees: f32,
    #[serde(default)]
    pub default_textbox_background: bool,
    #[serde(default)]
    pub pointer_mode: PointerMode,
    #[serde(default)]
    pub always_on_top: bool,
    /// Unix-epoch milliseconds of the most recent successful update
    /// discovery check. `None` means the app has not yet performed a
    /// check on this machine; the Settings Tab renders that as "never".
    #[serde(default)]
    pub last_update_check_at: Option<u64>,
}

fn default_auto_image_scaling() -> bool {
    true
}

fn default_import_gap() -> f32 {
    DEFAULT_IMPORT_GAP
}

fn default_import_target_canvas_area() -> f32 {
    DEFAULT_IMPORT_TARGET_CANVAS_AREA
}

fn default_canvas_image_max_side() -> u32 {
    DEFAULT_CANVAS_IMAGE_MAX_SIDE
}

fn default_semantic_search_enabled() -> bool {
    true
}

fn default_rotation_snap_degrees() -> f32 {
    DEFAULT_ROTATION_SNAP_DEGREES
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            auto_image_scaling: true,
            import_gap: DEFAULT_IMPORT_GAP,
            import_target_canvas_area: DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            canvas_image_max_side: DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            semantic_search_enabled: true,
            semantic_search_first_launch_prompt_seen: false,
            rotation_snap_degrees: DEFAULT_ROTATION_SNAP_DEGREES,
            default_textbox_background: false,
            pointer_mode: PointerMode::default(),
            always_on_top: false,
            last_update_check_at: None,
        }
    }
}

impl WorkspaceSettings {
    pub fn normalize_core_features(&mut self) {
        self.semantic_search_enabled = true;
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceSettings;

    #[test]
    fn normalizes_semantic_search_to_enabled_for_core_search() {
        let mut settings = WorkspaceSettings {
            semantic_search_enabled: false,
            ..Default::default()
        };

        settings.normalize_core_features();

        assert!(settings.semantic_search_enabled);
    }

    #[test]
    fn first_launch_semantic_search_prompt_starts_unseen() {
        let settings = WorkspaceSettings::default();

        assert!(!settings.semantic_search_first_launch_prompt_seen);
    }

    #[test]
    fn default_image_gap_is_zero_for_new_workspaces() {
        let settings = WorkspaceSettings::default();

        assert_eq!(settings.import_gap, 0.0);
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
