//! Persisted form of `PlacementContent`.
//!
//! Loaded content stores only the Asset id; the Asset row is joined back on
//! read. Older payloads named the resolution `source` field `intent`.

use crate::data::{Asset, ContentResolutionSource, PlacementContent};

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) enum StoredPlacementContent {
    Loaded {
        asset_id: i64,
    },
    Loading {
        #[serde(alias = "intent")]
        source: ContentResolutionSource,
        status_message: Option<String>,
    },
    Failed {
        #[serde(alias = "intent")]
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

    /// `load_asset` resolves the Asset for Loaded content.
    pub(crate) fn into_content(
        self,
        load_asset: impl FnOnce(i64) -> Result<Asset, String>,
    ) -> Result<PlacementContent, String> {
        Ok(match self {
            Self::Loaded { asset_id } => PlacementContent::Loaded {
                asset: load_asset(asset_id)?,
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
        })
    }
}
