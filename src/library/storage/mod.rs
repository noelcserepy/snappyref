mod support;
use support::*;

mod annotations;
mod assets;
mod boards;
mod connection;
mod embeddings;
mod files;
mod placements;
mod schema;
mod session;
mod stack_order;
#[cfg(test)]
mod tests;

pub use assets::ExactDedupeReport;
pub(crate) use assets::{sha256_bytes, sha256_file};
pub use connection::Library;
#[cfg(test)]
pub use embeddings::ActiveCanvasEmbedding;
#[cfg(feature = "semantic-search")]
pub use embeddings::{AssetSearchHit, EmbeddingStatus};
