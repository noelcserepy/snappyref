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
}

#[derive(Clone, Debug)]
pub struct BoardItemWithAsset {
    pub item: BoardItem,
    pub asset: super::ImageAsset,
}
