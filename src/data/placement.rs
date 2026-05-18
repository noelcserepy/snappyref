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
        Self {
            id,
            asset_id,
            geometry: RectGeometry::new(center, Vec2::ZERO, scale, rotation, z_index),
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
    pub visual_size: Vec2,
}

impl Placement {
    pub fn loaded(mut state: PlacementState, asset: super::ImageAsset) -> Self {
        let visual_size = vec2(asset.width as f32, asset.height as f32);
        state.asset_id = Some(asset.id);
        Self {
            content: PlacementContent::Loaded { asset_id: asset.id },
            lifecycle: PlacementLifecycle::Committed,
            visual_size,
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

    pub fn base_visual_size(&self) -> eframe::egui::Vec2 {
        vec2(self.visual_size.x.max(1.0), self.visual_size.y.max(1.0))
    }

    pub fn scaled_visual_size(&self) -> eframe::egui::Vec2 {
        self.base_visual_size() * self.state.geometry.scale.max(0.01)
    }

    pub fn visual_bounds(&self) -> Rect {
        let mut geometry = self.state.geometry;
        geometry.size = self.base_visual_size();
        geometry.visual_bounds()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacementFootprint {
    pub id: PlacementId,
    pub center: Pos2,
    pub width: f32,
    pub height: f32,
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
