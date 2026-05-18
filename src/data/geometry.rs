use eframe::egui::{Pos2, Rect, Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectGeometry {
    pub center: Pos2,
    pub size: Vec2,
    pub scale: f32,
    pub rotation: f32,
    pub z_index: i32,
}

impl RectGeometry {
    pub fn new(center: Pos2, size: Vec2, scale: f32, rotation: f32, z_index: i32) -> Self {
        Self {
            center,
            size,
            scale,
            rotation,
            z_index,
        }
    }

    pub fn from_bounds(bounds: Rect, z_index: i32) -> Self {
        Self::new(bounds.center(), bounds.size(), 1.0, 0.0, z_index)
    }

    pub fn scaled_size(self) -> Vec2 {
        vec2(self.size.x.max(1.0), self.size.y.max(1.0)) * self.scale.max(0.01)
    }

    pub fn unrotated_bounds(self) -> Rect {
        Rect::from_center_size(self.center, self.scaled_size())
    }

    pub fn visual_bounds(self) -> Rect {
        Rect::from_center_size(
            self.center,
            rotated_visual_size(self.scaled_size(), self.rotation),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentGeometry {
    pub start: Pos2,
    pub end: Pos2,
    pub z_index: i32,
}

impl SegmentGeometry {
    pub fn new(start: Pos2, end: Pos2, z_index: i32) -> Self {
        Self {
            start,
            end,
            z_index,
        }
    }
}

pub fn rotated_visual_size(size: Vec2, rotation: f32) -> Vec2 {
    let sin = rotation.sin().abs();
    let cos = rotation.cos().abs();
    vec2(size.x * cos + size.y * sin, size.x * sin + size.y * cos)
}
