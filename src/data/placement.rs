use eframe::egui::{Pos2, Rect, Vec2, vec2};

use super::RectGeometry;

pub type PlacementId = i64;
pub type AssetId = i64;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub id: i64,
    pub name: String,
    pub archived: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub struct BoardSummary {
    pub board: Board,
    pub placement_count: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PlacementState {
    pub id: PlacementId,
    pub asset_id: Option<AssetId>,
    pub geometry: RectGeometry,
    pub archived: bool,
}

impl PlacementState {
    pub fn new(
        id: PlacementId,
        asset_id: Option<AssetId>,
        center: Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
        archived: bool,
    ) -> Self {
        Self::with_display_size(
            id,
            asset_id,
            center,
            Vec2::ZERO,
            scale,
            rotation,
            z_index,
            archived,
        )
    }

    pub fn with_display_size(
        id: PlacementId,
        asset_id: Option<AssetId>,
        center: Pos2,
        size: Vec2,
        scale: f32,
        rotation: f32,
        z_index: i32,
        archived: bool,
    ) -> Self {
        Self {
            id,
            asset_id,
            geometry: RectGeometry::new(center, size, scale, rotation, z_index),
            archived,
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PlacementContent {
    Loaded {
        asset_id: i64,
    },
    Loading {
        #[serde(alias = "intent")]
        source: ContentResolutionSource,
        status_message: Option<String>,
    },
    Failed {
        #[serde(alias = "intent")]
        source: Option<ContentResolutionSource>,
        message: String,
        recoverable: bool,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ContentResolutionSource {
    ExternalPath { path: String },
    ExternalUrl { url: String },
    Clipboard,
    SearchAsset { asset_id: i64 },
    Unknown { description: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum PlacementLifecycle {
    Provisional,
    Committed,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct Placement {
    pub state: PlacementState,
    pub asset: Option<super::ImageAsset>,
    pub content: PlacementContent,
    pub lifecycle: PlacementLifecycle,
}

impl Placement {
    pub fn loaded(mut state: PlacementState, asset: super::ImageAsset) -> Self {
        state.asset_id = Some(asset.id);
        if state.geometry.size.x <= 0.0 || state.geometry.size.y <= 0.0 {
            state.geometry.size = vec2(asset.width as f32, asset.height as f32);
        }
        Self {
            content: PlacementContent::Loaded { asset_id: asset.id },
            lifecycle: PlacementLifecycle::Committed,
            state,
            asset: Some(asset),
        }
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self.content, PlacementContent::Loaded { .. })
    }

    pub fn id(&self) -> PlacementId {
        self.state.id
    }

    #[allow(dead_code)]
    pub fn asset_id(&self) -> Option<AssetId> {
        self.state.asset_id
    }

    #[allow(dead_code)]
    pub fn center(&self) -> Pos2 {
        self.state.geometry.center
    }

    pub fn base_display_size(&self) -> eframe::egui::Vec2 {
        vec2(
            self.state.geometry.size.x.max(1.0),
            self.state.geometry.size.y.max(1.0),
        )
    }

    pub fn scaled_display_size(&self) -> eframe::egui::Vec2 {
        self.state.geometry.scaled_size()
    }

    pub fn visual_bounds(&self) -> Rect {
        self.state.geometry.visual_bounds()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacementLayoutItem {
    pub id: PlacementId,
    pub geometry: RectGeometry,
}

impl PlacementLayoutItem {
    pub fn from_rect(id: PlacementId, rect: Rect) -> Self {
        Self {
            id,
            geometry: RectGeometry::from_bounds(rect, 0),
        }
    }

    pub fn new(id: PlacementId, center: Pos2, display_size: Vec2) -> Self {
        Self {
            id,
            geometry: RectGeometry::new(center, display_size, 1.0, 0.0, 0),
        }
    }

    pub fn center(&self) -> Pos2 {
        self.geometry.center
    }

    pub fn size(&self) -> Vec2 {
        self.geometry.size * self.geometry.scale.max(0.01)
    }

    pub fn rect(&self) -> Rect {
        Rect::from_center_size(self.center(), self.size())
    }

    pub fn width(&self) -> f32 {
        self.size().x
    }

    pub fn height(&self) -> f32 {
        self.size().y
    }

    pub fn with_size(&self, display_size: Vec2) -> Self {
        let mut item = self.clone();
        item.geometry.size = display_size;
        item.geometry.scale = 1.0;
        item
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CanvasStackItem {
    Placement(i64),
    Annotation(i64),
}

impl CanvasStackItem {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Placement(_) => "placement",
            Self::Annotation(_) => "annotation",
        }
    }

    pub fn item_id(self) -> i64 {
        match self {
            Self::Placement(item_id) | Self::Annotation(item_id) => item_id,
        }
    }

    pub fn from_parts(kind: &str, item_id: i64) -> Option<Self> {
        match kind {
            "placement" => Some(Self::Placement(item_id)),
            "annotation" => Some(Self::Annotation(item_id)),
            _ => None,
        }
    }
}
