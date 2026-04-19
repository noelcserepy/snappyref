use crate::storage::{AssetSearchHit, Library};

pub trait VectorIndex: Send + Sync {
    fn search(
        &self,
        model_key: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<AssetSearchHit>, String>;
}

pub struct BruteForceIndex {
    library: Library,
}

impl BruteForceIndex {
    pub fn new(library: Library) -> Self {
        Self { library }
    }
}

impl VectorIndex for BruteForceIndex {
    fn search(
        &self,
        model_key: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<AssetSearchHit>, String> {
        let mut hits = self
            .library
            .load_active_canvas_embeddings(model_key)?
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

fn cosine_similarity(left: &[f32], right: &[f32]) -> Result<f32, String> {
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
