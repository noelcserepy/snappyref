pub(crate) mod palette;
pub(crate) mod semantic;

mod backfill;
mod controller;
mod overlay;
mod placement;
mod selection;

pub(crate) use backfill::EmbeddingBackfillIdleState;
pub(crate) use controller::{
    SearchController, SearchDropRequest, SearchPlacementPreviewConfig, SemanticPerfEvent,
};
