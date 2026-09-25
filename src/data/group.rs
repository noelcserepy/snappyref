use super::{PlacementId, RectGeometry};
use std::collections::BTreeMap;

pub type GroupId = i64;

/// Ordered, flat membership and the saved layout intent. Geometry is in board pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub members: Vec<PlacementId>,
    pub container: RectGeometry,
    pub flow: GroupFlow,
    pub gap: f32,
    pub padding: f32,
    pub layout: GroupLayoutSettings,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroupLayoutSettings {
    pub fixed: [bool; 2],
    pub auto: GapMode,
    pub row_gap: f32,
    pub align: [usize; 2],
    pub clip: bool,
    pub members: BTreeMap<i64, MemberLayout>,
}
impl Default for GroupLayoutSettings {
    fn default() -> Self {
        Self {
            fixed: [true, false],
            auto: GapMode::Number,
            row_gap: 0.0,
            align: [0, 0],
            clip: false,
            members: BTreeMap::new(),
        }
    }
}
/// Number spaces flowing members by the Group gap. The others distribute leftover space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GapMode {
    #[default]
    Number,
    Between,
    Evenly,
    Around,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemberLayout {
    pub fill: [bool; 2],
    pub absolute: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GroupFlow {
    #[default]
    Wrap,
    Row,
    Column,
    Free,
}
impl GroupFlow {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wrap => "Wrap",
            Self::Row => "Row",
            Self::Column => "Column",
            Self::Free => "Free",
        }
    }
}
