use super::{CanvasAnnotation, Placement, ViewportState};

/// A non-archived Board. Archived rows never load, so the model carries no
/// archived flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Board {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoardSummary {
    pub board: Board,
    pub placement_count: usize,
}

#[derive(Clone, Debug)]
pub struct BoardDocumentSnapshot {
    pub board: Board,
    pub placements: Vec<Placement>,
    pub annotations: Vec<CanvasAnnotation>,
    pub groups: Vec<super::Group>,
    pub stack_order: Vec<CanvasStackItem>,
    pub viewport: ViewportState,
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
