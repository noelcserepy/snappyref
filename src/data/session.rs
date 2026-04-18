#[derive(Clone, Debug)]
pub struct UserSession {
    pub camera_pan_x: f32,
    pub camera_pan_y: f32,
    pub camera_zoom: f32,
    pub auto_image_scaling: bool,
}

impl Default for UserSession {
    fn default() -> Self {
        Self {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            auto_image_scaling: true,
        }
    }
}
