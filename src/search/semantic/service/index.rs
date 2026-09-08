use std::sync::{Arc, Mutex};
use std::thread;

use crate::library::storage::{AssetSearchHit, Library};

#[derive(Clone, Debug)]
struct DecodedAssetEmbedding {
    asset_id: i64,
    embedding: Vec<f32>,
    cosine_is_zero: bool,
}

#[derive(Default)]
struct SearchEmbeddingCache {
    library_embeddings: Option<Arc<Vec<DecodedAssetEmbedding>>>,
    warming: bool,
    generation: u64,
}

/// Owns decoded Asset embeddings, cache generations, and ranking. Clones share the cache.
#[derive(Clone)]
pub(super) struct AssetIndex {
    library: Library,
    model_key: &'static str,
    cache: Arc<Mutex<SearchEmbeddingCache>>,
}

impl AssetIndex {
    pub(super) fn new(library: Library, model_key: &'static str) -> Self {
        Self {
            library,
            model_key,
            cache: Arc::new(Mutex::new(SearchEmbeddingCache::default())),
        }
    }
    pub(super) fn contains(&self, asset_id: i64) -> Result<bool, String> {
        Ok(self
            .library
            .load_asset_embedding(asset_id, self.model_key)?
            .is_some())
    }

    pub(super) fn is_warming(&self) -> bool {
        self.cache
            .lock()
            .map(|cache| cache.warming)
            .unwrap_or(false)
    }
    pub(super) fn pivot_embedding(&self, asset_id: i64) -> Result<Vec<f32>, String> {
        let embeddings = self.embeddings()?;
        embeddings
            .binary_search_by_key(&asset_id, |row| row.asset_id)
            .map(|index| embeddings[index].embedding.clone())
            .map_err(|_| "Source image not indexed yet".to_owned())
    }
    pub(super) fn rerank(&self, hits: &mut [AssetSearchHit], query: &[f32]) -> Result<(), String> {
        let embeddings = self.embeddings()?;
        let query_is_zero = squared_norm(query) <= f32::EPSILON;
        let query = normalized_embedding(query);
        for hit in hits.iter_mut() {
            if let Ok(index) = embeddings.binary_search_by_key(&hit.asset_id, |row| row.asset_id) {
                let row = &embeddings[index];
                let similarity = dot_product(&query, &row.embedding)?;
                hit.similarity = if query_is_zero || row.cosine_is_zero {
                    0.0
                } else {
                    similarity
                };
            }
        }
        hits.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
        Ok(())
    }
    pub(super) fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<AssetSearchHit>, String> {
        let embeddings = self.embeddings()?;
        score_decoded_embeddings(&embeddings, query_embedding, limit)
    }

    fn embeddings(&self) -> Result<Arc<Vec<DecodedAssetEmbedding>>, String> {
        loop {
            let generation = {
                let cache = self
                    .cache
                    .lock()
                    .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?;
                if let Some(embeddings) = &cache.library_embeddings {
                    return Ok(embeddings.clone());
                }
                cache.generation
            };
            let embeddings = load_decoded_library_embeddings(&self.library, self.model_key)?;
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?;
            if cache.generation != generation {
                continue;
            }
            cache.warming = false;
            return Ok(cache.library_embeddings.get_or_insert(embeddings).clone());
        }
    }

    pub(super) fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.library_embeddings = None;
            cache.warming = false;
            cache.generation = cache.generation.wrapping_add(1);
        }
    }

    pub(super) fn refresh(&self) {
        self.invalidate();
        let generation = match self.cache.lock() {
            Ok(mut cache) => {
                if cache.library_embeddings.is_some() || cache.warming {
                    return;
                }
                cache.warming = true;
                cache.generation
            }
            Err(_) => return,
        };

        let library = self.library.clone();
        let model_key = self.model_key;
        let cache = self.cache.clone();
        thread::spawn(move || {
            let decoded_embeddings = load_decoded_library_embeddings(&library, model_key);
            let Ok(mut cache) = cache.lock() else {
                return;
            };
            if cache.generation != generation {
                return;
            }
            cache.warming = false;
            if let Ok(decoded_embeddings) = decoded_embeddings {
                cache.library_embeddings = Some(decoded_embeddings);
            }
        });
    }
}
fn load_decoded_library_embeddings(
    library: &Library,
    model_key: &'static str,
) -> Result<Arc<Vec<DecodedAssetEmbedding>>, String> {
    library
        .load_library_embeddings(model_key)?
        .into_iter()
        .map(|row| {
            let embedding = decode_f32_blob(&row.embedding_blob)?;
            Ok(DecodedAssetEmbedding {
                asset_id: row.asset_id,
                cosine_is_zero: squared_norm(&embedding) <= f32::EPSILON,
                embedding: normalized_embedding(&embedding),
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Arc::new)
}

fn score_decoded_embeddings(
    embeddings: &[DecodedAssetEmbedding],
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let query_embedding = normalized_embedding(query_embedding);
    let mut hits = embeddings
        .iter()
        .map(|row| {
            Ok(AssetSearchHit {
                asset_id: row.asset_id,
                similarity: dot_product(&query_embedding, &row.embedding)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    hits.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
    hits.truncate(limit);
    Ok(hits)
}

fn squared_norm(embedding: &[f32]) -> f32 {
    embedding.iter().map(|value| value * value).sum()
}

fn normalized_embedding(embedding: &[f32]) -> Vec<f32> {
    let norm = squared_norm(embedding).sqrt();
    if norm <= f32::EPSILON {
        return vec![0.0; embedding.len()];
    }
    embedding.iter().map(|value| value / norm).collect()
}

fn dot_product(left: &[f32], right: &[f32]) -> Result<f32, String> {
    if left.len() != right.len() {
        return Err("embedding dimensions do not match".to_owned());
    }
    Ok(left
        .iter()
        .zip(right.iter())
        .map(|(lhs, rhs)| lhs * rhs)
        .sum())
}

fn decode_f32_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
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
