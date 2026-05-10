mod index;
mod model;
mod service;

pub use index::SearchScope;
#[allow(unused_imports)]
pub use model::AVAILABLE_SEMANTIC_SEARCH_MODELS;
pub use model::DEFAULT_SEMANTIC_SEARCH_MODEL;
pub(crate) use model::{ImageEmbeddingTiming, OrtDualEncoderModel};
pub use service::{
    IndexResult, SearchQueryResult, SearchResultGroup, SemanticSearchConfig, SemanticSearchService,
};
