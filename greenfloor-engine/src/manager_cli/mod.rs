//! Native `GreenFloor` manager CLI (`greenfloor-manager` binary).

mod cats;
mod cats_catalog;
mod coin_op_loop;
mod combine_market_cat_dust;
mod commands;
mod context;
mod dispatch;
mod flag_groups;
mod json;
mod keys;
mod ladder;
mod offers;
mod paths;
mod runtime;
mod setup;
mod util;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;

pub use cats_catalog::load_cats_catalog;
pub use commands::{ManagerCli, ManagerCommands};
pub use dispatch::run_manager_cli;
pub use offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
pub use paths::{
    default_cats_config_path, default_markets_config_path, default_metadata_config_paths,
    default_program_config_path, default_testnet_markets_config_path,
    default_vault_scan_metadata_config_paths, optional_path, program_config_path_from_optional,
};
