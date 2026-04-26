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

#[derive(Clone, Debug)]
pub struct BoardItemWithAsset {
    pub item: BoardItem,
    pub asset: super::ImageAsset,
}

#[derive(Clone, Debug)]
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
