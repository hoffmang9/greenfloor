//! Shared operator orchestration for manager CLI and daemon offer dispatch.

mod signer_denomination;
mod logging;
mod build_and_post;
mod test_overrides;

pub use signer_denomination::{
    bootstrap_blocks_offer, BootstrapPhaseResult,
};
pub use build_and_post::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostOfferResponse,
};
pub use logging::{initialize_manager_file_logging, warn_if_log_level_auto_healed};
pub use test_overrides::OfferOperatorTestOverrides;
