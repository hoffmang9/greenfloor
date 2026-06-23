//! Shared operator orchestration for manager CLI and daemon offer dispatch.

mod build_and_post;
mod logging;
mod signer_denomination;
#[cfg(test)]
mod test_overrides;

#[cfg(test)]
pub(crate) use build_and_post::empty_persist_artifacts_for_test;
pub use build_and_post::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostOfferRequestParts,
    BuildAndPostOfferResponse, BuildAndPostRunOptions, BuildAndPostVenueOptions,
};
pub(crate) use build_and_post::{
    build_and_post_offer_with_persist_artifacts, flush_build_and_post_persist,
};
pub use logging::{
    initialize_manager_file_logging, sync_manager_file_logging, warn_if_log_level_auto_healed,
};
pub use signer_denomination::BootstrapPhaseResult;
#[cfg(test)]
pub use test_overrides::BuildOfferTestOverrides;
