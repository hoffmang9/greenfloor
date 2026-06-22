//! Expected offered/requested assets for post-publish Dexie visibility checks.

use serde::{Deserialize, Serialize};

use crate::offer::request::offer_side_assets_for_side;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishAssetSide {
    pub asset_id: String,
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedPublishAssetFields {
    pub offered: PublishAssetSide,
    pub requested: PublishAssetSide,
}

/// Resolve expected offered/requested assets for Dexie visibility checks.
#[must_use]
pub fn expected_publish_asset_fields(
    side: &str,
    base_symbol: &str,
    quote_asset: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> ExpectedPublishAssetFields {
    let assets = offer_side_assets_for_side(
        side,
        base_symbol,
        quote_asset,
        resolved_base_asset_id,
        resolved_quote_asset_id,
    );
    ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: assets.offered_asset_id,
            symbol: assets.offered_symbol,
        },
        requested: PublishAssetSide {
            asset_id: assets.requested_asset_id,
            symbol: assets.requested_symbol,
        },
    }
}
