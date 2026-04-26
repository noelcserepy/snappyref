use super::CanvasBackground;

pub const DEFAULT_IMPORT_GAP: f32 = 24.0;
pub const DEFAULT_CANVAS_IMAGE_MAX_SIDE: u32 = 2000;
pub const DEFAULT_ROTATION_SNAP_DEGREES: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CanvasImageCacheTier {
    Overview,
    Working,
    Detail,
}

impl CanvasImageCacheTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Working => "working",
            Self::Detail => "detail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasImageMode {
    Downscale,
    Original,
}

impl CanvasImageMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Downscale => "downscale",
            Self::Original => "original",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "downscale" => Some(Self::Downscale),
            "original" => Some(Self::Original),
            _ => None,
        }
    }
}

impl Default for CanvasImageMode {
    fn default() -> Self {
        Self::Downscale
    }
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct WorkspaceSettings {
    pub auto_image_scaling: bool,
    pub import_gap: f32,
    pub canvas_image_mode: CanvasImageMode,
    pub canvas_image_max_side: u32,
    pub semantic_search_enabled: bool,
    pub rotation_snap_degrees: f32,
    pub default_textbox_background: bool,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            auto_image_scaling: true,
            import_gap: DEFAULT_IMPORT_GAP,
            canvas_image_mode: CanvasImageMode::default(),
            canvas_image_max_side: DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            semantic_search_enabled: true,
            rotation_snap_degrees: DEFAULT_ROTATION_SNAP_DEGREES,
            default_textbox_background: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserSession {
    pub active_board_id: i64,
    pub viewport: ViewportState,
    pub settings: WorkspaceSettings,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            active_board_id: 1,
            viewport: ViewportState::default(),
            settings: WorkspaceSettings::default(),
        }
    }
}
