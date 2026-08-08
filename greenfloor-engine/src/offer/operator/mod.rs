//! Shared operator orchestration for manager CLI and daemon offer dispatch.

mod build_and_post;
mod ensure_size;
mod logging;
mod signer_denomination;
#[cfg(test)]
mod test_overrides;
mod unique_maker;

#[cfg(test)]
pub(crate) use build_and_post::empty_persist_artifacts_for_test;
pub use build_and_post::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostOfferRequestParts,
    BuildAndPostOfferResponse, BuildAndPostRunOptions, BuildAndPostVenueOptions,
    OperatorConfigPaths,
};
pub(crate) use build_and_post::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
};
pub use ensure_size::ensure_size_n_offer;
pub use logging::{
    initialize_manager_file_logging, sync_manager_file_logging, warn_if_log_level_auto_healed,
};
pub use signer_denomination::BootstrapPhaseResult;
#[cfg(test)]
pub(crate) use signer_denomination::BootstrapShapeContext;
#[cfg(test)]
pub(crate) use signer_denomination::SignerDenominationTestOverrides;
#[cfg(test)]
pub use test_overrides::BuildOfferTestOverrides;
pub(crate) use unique_maker::{
    load_binding_maker_coin_ids, needs_live_unique_pin, record_session_pin,
    resolve_unique_offer_coin_ids, session_excludes_for_unique_pin,
};
