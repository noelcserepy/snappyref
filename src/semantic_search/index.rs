use crate::storage::{ActiveCanvasEmbedding, AssetSearchHit, Library};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchScope {
    ThisBoard,
    Library,
}

impl Default for SearchScope {
    fn default() -> Self {
        Self::ThisBoard
    }
}

impl SearchScope {
    pub(crate) fn includes_unplaced_assets(self) -> bool {
        matches!(self, Self::Library)
    }
}

pub fn search_embeddings_for_scope(
    library: &Library,
    scope: SearchScope,
    model_key: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let embeddings = match scope {
        SearchScope::ThisBoard => library.load_active_canvas_embeddings(model_key)?,
        SearchScope::Library => library.load_library_embeddings(model_key)?,
    };
    search_embedding_rows(embeddings, query_embedding, limit)
}

fn search_embedding_rows(
    embeddings: Vec<ActiveCanvasEmbedding>,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let mut hits = embeddings
        .into_iter()
        .map(|row| {
            let embedding = decode_f32_blob(&row.embedding_blob)?;
            let similarity = cosine_similarity(query_embedding, &embedding)?;
            Ok(AssetSearchHit {
                asset_id: row.asset_id,
                similarity,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    hits.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
    hits.truncate(limit);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::{SearchScope, search_embeddings_for_scope};
    use crate::storage::Library;
    use eframe::egui;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn library_scope_search_includes_assets_without_active_placements() {
        let root = temp_root("reference-library-scope-index-test");
        let library = Library::initialize(root.clone()).unwrap();
        let active_path = library
            .store_rgba_image("active", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let unplaced_path = library
            .store_rgba_image("unplaced", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let active = library
            .persist_imported_image(
                &active_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let unplaced = library
            .persist_imported_image(
                &unplaced_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        library.archive_board_items(&[unplaced.item.id]).unwrap();
        library
            .upsert_asset_embedding_blob(active.asset.id, "test-model", &[0.0, 1.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(unplaced.asset.id, "test-model", &[1.0, 0.0])
            .unwrap();

        let board_hits = search_embeddings_for_scope(
            &library,
            SearchScope::ThisBoard,
            "test-model",
            &[1.0, 0.0],
            5,
        )
        .unwrap();
        let library_hits = search_embeddings_for_scope(
            &library,
            SearchScope::Library,
            "test-model",
            &[1.0, 0.0],
            5,
        )
        .unwrap();

        assert_eq!(board_hits.len(), 1);
        assert_eq!(board_hits[0].asset_id, active.asset.id);
        assert_eq!(library_hits[0].asset_id, unplaced.asset.id);
        assert!(
            library_hits
                .iter()
                .any(|hit| hit.asset_id == active.asset.id)
        );

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{}-{}",
            prefix,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}

pub fn decode_f32_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
    if !blob.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err("embedding blob has invalid length".to_owned());
    }

    blob.chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            let bytes: [u8; 4] = chunk
                .try_into()
                .map_err(|_| "embedding blob chunk decode failed".to_owned())?;
            Ok(f32::from_ne_bytes(bytes))
        })
        .collect()
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32, String> {
    if left.len() != right.len() {
        return Err("embedding dimensions do not match".to_owned());
    }

    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        dot += lhs * rhs;
        left_norm += lhs * lhs;
        right_norm += rhs * rhs;
    }

    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return Ok(0.0);
    }

    Ok(dot / (left_norm.sqrt() * right_norm.sqrt()))
}
