use eframe::egui::{Pos2, Rect, Vec2, vec2};

use super::RectGeometry;

pub type PlacementId = i64;
pub type AssetId = i64;

#[derive(Clone, Debug)]
pub struct PlacementState {
    pub id: PlacementId,
    pub geometry: RectGeometry,
    pub archived: bool,
}

impl PlacementState {
    #[cfg(test)]
    pub fn new(
        id: PlacementId,
        center: Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
        archived: bool,
    ) -> Self {
        Self::with_display_size(id, center, Vec2::ZERO, scale, rotation, z_index, archived)
    }

    pub fn with_display_size(
        id: PlacementId,
        center: Pos2,
        size: Vec2,
        scale: f32,
        rotation: f32,
        z_index: i32,
        archived: bool,
    ) -> Self {
        Self {
            id,
            geometry: RectGeometry::new(center, size, scale, rotation, z_index),
            archived,
        }
    }
}

/// What a board-local Placement currently renders and resolves to.
///
/// A Placement is the canonical canvas object even while content is still
/// loading or has failed. Async import/search/clipboard work updates this
/// state in place so selection, movement, history, and stack order do not
/// need a separate placeholder model. The persisted form lives in the
/// library storage adapter.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacementContent {
    Loaded {
        asset: super::Asset,
    },
    Loading {
        source: ContentResolutionSource,
        status_message: Option<String>,
    },
    Failed {
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

/// Whether a Placement is already part of the persisted board document or is
/// a preview that can still be canceled without a history entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum PlacementLifecycle {
    Provisional,
    Committed,
}

/// Board-local visual object. Loaded Placements carry their Library Asset;
/// loading/failed Placements carry the source needed to resolve or explain
/// their content.
#[derive(Clone, Debug)]
pub struct Placement {
    pub state: PlacementState,
    pub content: PlacementContent,
    pub lifecycle: PlacementLifecycle,
}

impl Placement {
    pub fn loaded(mut state: PlacementState, asset: super::Asset) -> Self {
        if state.geometry.size.x <= 0.0 || state.geometry.size.y <= 0.0 {
            state.geometry.size = vec2(asset.width as f32, asset.height as f32);
        }
        Self {
            content: PlacementContent::Loaded { asset },
            lifecycle: PlacementLifecycle::Committed,
            state,
        }
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self.content, PlacementContent::Loaded { .. })
    }

    /// The Library Asset this Placement renders, or is still resolving
    /// toward when it was placed from a known Asset.
    pub fn asset_id(&self) -> Option<AssetId> {
        match &self.content {
            PlacementContent::Loaded { asset } => Some(asset.id),
            PlacementContent::Loading {
                source: ContentResolutionSource::SearchAsset { asset_id },
                ..
            } => Some(*asset_id),
            _ => None,
        }
    }

    /// Adopts the geometry of the record already on the Board. An image load
    /// can finish after the user moved, rotated, or restacked the Placement,
    /// and that newer geometry wins. Size and scale follow only once the same
    /// Asset was already loaded; a first load fits the Asset's aspect.
    pub fn adopt_geometry_of(&mut self, current: &Placement) {
        let newer = current.state.geometry;
        let same_loaded_asset = current.is_loaded() && current.asset_id() == self.asset_id();
        let geometry = &mut self.state.geometry;
        geometry.center = newer.center;
        geometry.rotation = newer.rotation;
        geometry.z_index = newer.z_index;
        if same_loaded_asset {
            geometry.size = newer.size;
            geometry.scale = newer.scale;
        }
    }

    pub fn loaded_asset(&self) -> Option<&super::Asset> {
        match &self.content {
            PlacementContent::Loaded { asset } => Some(asset),
            _ => None,
        }
    }

    pub fn require_loaded_asset(&self) -> &super::Asset {
        self.loaded_asset()
            .expect("loaded Placement should carry its Asset")
    }

    #[cfg(test)]
    pub fn require_loaded_asset_mut(&mut self) -> &mut super::Asset {
        match &mut self.content {
            PlacementContent::Loaded { asset } => asset,
            _ => panic!("loaded Placement should carry its Asset"),
        }
    }

    pub fn id(&self) -> PlacementId {
        self.state.id
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
