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
pub struct BoardItem {
    pub id: i64,
    pub asset_id: i64,
    pub x: f32,
    pub y: f32,
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
        intent: PlacementContentIntent,
        status_message: Option<String>,
    },
    Failed {
        intent: Option<PlacementContentIntent>,
        message: String,
        recoverable: bool,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PlacementContentIntent {
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
pub struct BoardItemWithAsset {
    pub item: BoardItem,
    pub asset: super::ImageAsset,
    pub content: PlacementContent,
    pub lifecycle: PlacementLifecycle,
    pub visual_width: f32,
    pub visual_height: f32,
}

impl BoardItemWithAsset {
    pub fn loaded(item: BoardItem, asset: super::ImageAsset) -> Self {
        let visual_width = asset.width as f32;
        let visual_height = asset.height as f32;
        Self {
            content: PlacementContent::Loaded { asset_id: asset.id },
            lifecycle: PlacementLifecycle::Committed,
            visual_width,
            visual_height,
            item,
            asset,
        }
    }

    pub fn placeholder_asset(
        item_id: i64,
        filename: impl Into<String>,
        width: u32,
        height: u32,
    ) -> super::ImageAsset {
        let filename = filename.into();
        super::ImageAsset {
            id: 0_i64.saturating_sub(item_id.abs()),
            original_path: std::path::PathBuf::from(&filename),
            filename,
            mime_type: "application/x-snappyref-placeholder".to_owned(),
            width,
            height,
            file_size: 0,
            imported_at: 0,
        }
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self.content, PlacementContent::Loaded { .. })
    }

    pub fn base_visual_size(&self) -> eframe::egui::Vec2 {
        eframe::egui::vec2(self.visual_width.max(1.0), self.visual_height.max(1.0))
    }

    pub fn scaled_visual_size(&self) -> eframe::egui::Vec2 {
        self.base_visual_size() * self.item.scale.max(0.01)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardLayoutItem {
    pub id: i64,
    pub x: f32,
    pub y: f32,
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
