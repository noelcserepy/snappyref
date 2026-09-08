//! Persisted form of `PlacementContent`.
//!
//! Loaded content stores only the Asset id; the Asset row is joined back on
//! read. Schema migration upgrades older source field names before normal reads.

use crate::data::{Asset, ContentResolutionSource, PlacementContent};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) enum StoredPlacementContent {
    Loaded {
        asset_id: i64,
    },
    Loading {
        source: ContentResolutionSource,
        status_message: Option<String>,
    },
    Failed {
        source: Option<ContentResolutionSource>,
        message: String,
        recoverable: bool,
    },
}

impl StoredPlacementContent {
    pub(crate) fn from_content(content: &PlacementContent) -> Self {
        match content {
            PlacementContent::Loaded { asset } => Self::Loaded { asset_id: asset.id },
            PlacementContent::Loading {
                source,
                status_message,
            } => Self::Loading {
                source: source.clone(),
                status_message: status_message.clone(),
            },
            PlacementContent::Failed {
                source,
                message,
                recoverable,
            } => Self::Failed {
                source: source.clone(),
                message: message.clone(),
                recoverable: *recoverable,
            },
        }
    }

    pub(crate) fn encode(content: &PlacementContent) -> Result<String, String> {
        serde_json::to_string(&Self::from_content(content))
            .map_err(|error| format!("failed to encode placement content: {error}"))
    }

    pub(crate) fn decode(payload: &str) -> Option<Self> {
        serde_json::from_str(payload).ok()
    }

    /// `asset` resolves the live Asset for Loaded content. Loaded content
    /// whose Asset is gone degrades to a recoverable failure instead of a
    /// Placement without an image.
    pub(crate) fn into_content(self, asset: impl FnOnce(i64) -> Option<Asset>) -> PlacementContent {
        match self {
            Self::Loaded { asset_id } => match asset(asset_id) {
                Some(asset) => PlacementContent::Loaded { asset },
                None => missing_asset_content(),
            },
            Self::Loading {
                source,
                status_message,
            } => PlacementContent::Loading {
                source,
                status_message,
            },
            Self::Failed {
                source,
                message,
                recoverable,
            } => PlacementContent::Failed {
                source,
                message,
                recoverable,
            },
        }
    }
}

pub(super) fn missing_asset_content() -> PlacementContent {
    PlacementContent::Failed {
        source: None,
        message: "Placement content failed to resolve".to_owned(),
        recoverable: true,
    }
}
