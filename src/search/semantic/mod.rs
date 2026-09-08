mod install;
mod install_controller;
mod model;
mod model_store;
mod service;

pub(crate) use install::SemanticSearchInstallProgress;
#[cfg(not(test))]
pub(crate) use install::remove_semantic_search_assets;
pub(crate) use install::{default_semantic_search_asset_descriptor, install_semantic_search_asset};
pub(crate) use install_controller::{
    SemanticSearchInstallController, SemanticSearchInstallDisplay, SemanticSearchInstallSnapshot,
};
pub use model::DEFAULT_SEMANTIC_SEARCH_MODEL;
pub(crate) use model::default_local_model_store_status;
pub(crate) use model_store::LocalModelStoreStatus;
pub use service::{
    IndexResult, SearchQueryResult, SearchResultAssetEntry, SemanticSearchConfig,
    SemanticSearchService,
};

pub(crate) use service::run_embed_image_server;
