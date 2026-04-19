use super::CanvasBackground;

pub const DEFAULT_IMPORT_GAP: f32 = 12.0;
pub const DEFAULT_CANVAS_IMAGE_MAX_SIDE: u32 = 2000;

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
pub struct UserSession {
    pub camera_pan_x: f32,
    pub camera_pan_y: f32,
    pub camera_zoom: f32,
    pub auto_image_scaling: bool,
    pub import_gap: f32,
    pub canvas_image_mode: CanvasImageMode,
    pub canvas_image_max_side: u32,
    pub canvas_background: CanvasBackground,
    pub semantic_search_enabled: bool,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            auto_image_scaling: true,
            import_gap: DEFAULT_IMPORT_GAP,
            canvas_image_mode: CanvasImageMode::default(),
            canvas_image_max_side: DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            canvas_background: CanvasBackground::default(),
            semantic_search_enabled: true,
        }
    }
}
