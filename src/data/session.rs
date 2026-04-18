pub const DEFAULT_IMPORT_GAP: f32 = 12.0;

#[derive(Clone, Debug)]
pub struct UserSession {
    pub camera_pan_x: f32,
    pub camera_pan_y: f32,
    pub camera_zoom: f32,
    pub auto_image_scaling: bool,
    pub import_gap: f32,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            auto_image_scaling: true,
            import_gap: DEFAULT_IMPORT_GAP,
        }
    }
}
