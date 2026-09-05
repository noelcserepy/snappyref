use super::{PlacementId, RectGeometry};

pub type GroupId = i64;

/// Ordered Board-local membership and the saved auto-layout configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub members: Vec<PlacementId>,
    pub container: RectGeometry,
    pub flow: GroupFlow,
    pub spacing: f32,
    pub padding: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GroupFlow {
    #[default]
    Wrap,
    Row,
    Column,
}
