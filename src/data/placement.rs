use eframe::egui::{Pos2, Rect, Vec2, vec2};

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
    pub image_count: usize,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PlacementState {
    pub id: PlacementId,
    pub asset_id: Option<AssetId>,
    pub center: Pos2,
    pub scale: f32,
    pub rotation: f32,
    pub z_index: i32,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PlacementContent {
    Loaded {
        asset_id: i64,
    },
    Loading {
        intent: ContentResolutionSource,
        status_message: Option<String>,
    },
    Failed {
        intent: Option<ContentResolutionSource>,
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
        self.state.center
    }

    pub fn base_visual_size(&self) -> eframe::egui::Vec2 {
        vec2(self.visual_size.x.max(1.0), self.visual_size.y.max(1.0))
    }

    pub fn scaled_visual_size(&self) -> eframe::egui::Vec2 {
        self.base_visual_size() * self.state.scale.max(0.01)
    }

    pub fn visual_bounds(&self) -> Rect {
        let size = rotated_visual_size(
            self.scaled_visual_size().x,
            self.scaled_visual_size().y,
            self.state.rotation,
        );
        Rect::from_center_size(self.state.center, size)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacementLayoutItem {
    pub id: PlacementId,
    pub center: Pos2,
    pub width: f32,
    pub height: f32,
}

fn rotated_visual_size(width: f32, height: f32, rotation: f32) -> Vec2 {
    let sin = rotation.sin().abs();
    let cos = rotation.cos().abs();
    vec2(width * cos + height * sin, width * sin + height * cos)
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
