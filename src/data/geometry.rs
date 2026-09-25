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

    // Fill may collapse an axis to zero; preserve that result through paint and storage.
    pub fn scaled_size(self) -> Vec2 {
        vec2(self.size.x.max(0.0), self.size.y.max(0.0)) * self.scale.max(0.01)
    }

    pub fn size_preserving_area_for_aspect(size: Vec2, width: u32, height: u32) -> Vec2 {
        let aspect = width.max(1) as f32 / height.max(1) as f32;
        let area = (size.x.max(1.0) * size.y.max(1.0)).max(1.0);
        vec2((area * aspect).sqrt(), (area / aspect).sqrt())
    }

    pub fn unrotated_bounds(self) -> Rect {
        Rect::from_center_size(self.center, self.scaled_size())
    }

    pub fn visual_bounds(self) -> Rect {
        Rect::from_center_size(
            self.center,
            rotated_display_size(self.scaled_size(), self.rotation),
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

pub fn rotated_display_size(size: Vec2, rotation: f32) -> Vec2 {
    let sin = rotation.sin().abs();
    let cos = rotation.cos().abs();
    vec2(size.x * cos + size.y * sin, size.x * sin + size.y * cos)
}
