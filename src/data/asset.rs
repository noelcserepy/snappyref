use std::path::PathBuf;

/// A library-owned image reference, independent of any board-local Placement.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Asset {
    pub id: i64,
    pub original_path: PathBuf,
    pub filename: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub imported_at: i64,
}
