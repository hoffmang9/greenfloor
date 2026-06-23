//! Shared [`BuildAndPostOfferRequest`] fixtures for operator tests.

use std::path::Path;

use crate::offer::operator::{
    BuildAndPostOfferRequest, BuildAndPostRunOptions, BuildAndPostVenueOptions,
    BuildOfferTestOverrides,
};

#[must_use]
pub fn post_iteration_request(
    program_path: &Path,
    markets_path: &Path,
    dry_run: bool,
    offer_text: Option<&str>,
) -> BuildAndPostOfferRequest {
    BuildAndPostOfferRequest {
        program_path: program_path.to_path_buf(),
        markets_path: markets_path.to_path_buf(),
        testnet_markets_path: None,
        network: "mainnet".to_string(),
        market_id: Some("m1".to_string()),
        pair: None,
        size_base_units: 10,
        repeat: 1,
        publish_venue: None,
        dexie_base_url: None,
        splash_base_url: None,
        venue: BuildAndPostVenueOptions {
            drop_only: true,
            claim_rewards: false,
        },
        run: BuildAndPostRunOptions {
            dry_run,
            persist_results: false,
            persist_store: None,
        },
        action_side: None,
        test_overrides: BuildOfferTestOverrides {
            offer_text: offer_text.map(str::to_string),
        },
    }
}

#[must_use]
pub fn unused_post_iteration_request(
    dry_run: bool,
    offer_text: Option<&str>,
) -> BuildAndPostOfferRequest {
    post_iteration_request(
        Path::new("/tmp/unused-program.yaml"),
        Path::new("/tmp/unused-markets.yaml"),
        dry_run,
        offer_text,
    )
}
