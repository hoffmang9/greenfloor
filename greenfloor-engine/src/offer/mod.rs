//! Offer build, validation, bootstrap planning, and publish policy.
//!
//! Offer construction, validation, and deterministic offer policy.

pub mod action;
pub mod assemble;
pub mod bootstrap;
pub mod build;
pub mod build_context;
mod cancel_input;
pub mod codec;
pub mod dexie_payload;
pub mod invariants;
pub mod lifecycle;
pub mod operator;
pub mod plan;
pub mod presplit;
pub mod pricing;
pub mod publish;
pub mod reclaim;
pub mod request;
pub mod types;

pub use action::{
    build_signer_offer_for_action, expires_at_unix_from_pricing, resolve_market_base_asset_id,
    resolve_market_offer_assets_for_action, resolve_market_offer_fee_asset_id,
    resolve_offer_assets_for_action, resolve_offer_assets_via_coinset,
    try_normalize_resolved_assets, BuildOfferForActionRequest, BuildOfferForActionResult,
    ResolvedMarketOfferAssets,
};
pub use bootstrap::{
    bootstrap_early_phase, bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCoin,
    BootstrapPhaseSnapshot, BootstrapPlan, BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
};
pub use build::build_vault_cat_offer;
pub use build_context::{
    mojo_multiplier_for_leg, resolve_offer_expiry_for_pricing, resolve_quote_price_for_pricing,
};
#[cfg(test)]
pub(crate) use cancel_input::classify_cancellable_maker_input;
pub use cancel_input::OfferReclaimMode;
pub use codec::{
    encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_bytes,
    from_input_spend_bundle_xch_bytes, validate_offer_structure, validate_offer_text,
    verify_offer_for_dexie,
};
pub use pricing::quote_mojos_for_base_size;
pub use publish::{
    expected_publish_asset_fields, post_offer_phase_dexie, ExpectedPublishAssetFields,
    PostOfferPhaseDexieParams, PublishAssetSide,
};
pub use reclaim::{build_offer_cancel_spend_bundle, build_vault_cat_reclaim_spend_bundle};
pub use request::{
    compute_signer_offer_leg_amounts, normalize_offer_asset_id, normalize_offer_side,
    signer_split_asset_id, SignerOfferLegAmounts,
};
pub use types::{
    CreateOfferRequest, CreateOfferResult, OfferExecutionMode, PresplitCancelFields,
    StoredOfferCancelMetadata,
};
