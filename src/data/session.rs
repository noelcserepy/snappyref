use super::CanvasBackground;

pub const DEFAULT_IMPORT_GAP: f32 = 24.0;
pub const DEFAULT_CANVAS_IMAGE_MAX_SIDE: u32 = 2000;
pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = 90_000.0;
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

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSettings {
    pub auto_image_scaling: bool,
    pub import_gap: f32,
    pub import_target_canvas_area: f32,
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
            import_target_canvas_area: DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            canvas_image_max_side: DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            semantic_search_enabled: true,
            rotation_snap_degrees: DEFAULT_ROTATION_SNAP_DEGREES,
            default_textbox_background: false,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserSession {
    pub active_board_id: i64,
    pub viewport: ViewportState,
    pub settings: WorkspaceSettings,
    pub sidebar_open: bool,
    pub sidebar_tab: String,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            active_board_id: 1,
            viewport: ViewportState::default(),
            settings: WorkspaceSettings::default(),
            sidebar_open: false,
            sidebar_tab: "settings".to_owned(),
        }
    }
}
