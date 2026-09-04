mod support;
use support::*;

mod annotations;
mod assets;
mod boards;
mod connection;
mod durability;
pub(crate) use durability::BoardDurabilityOperation;
mod embeddings;
mod files;
mod placement_content;
pub(crate) use placement_content::StoredPlacementContent;
mod placements;
mod schema;
mod session;
mod stack_order;
#[cfg(test)]
mod tests;

pub use assets::ExactDedupeReport;
pub(crate) use assets::{sha256_bytes, sha256_file};
pub use connection::Library;
pub use embeddings::{AssetSearchHit, EmbeddingStatus};
