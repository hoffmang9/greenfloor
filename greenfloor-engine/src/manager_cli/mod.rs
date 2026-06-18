//! Native GreenFloor manager CLI (`greenfloor-manager` binary).

mod cats;
mod coin_op_loop;
mod commands;
mod dispatch;
mod json;
mod keys;
mod ladder;
mod offers;
mod paths;
mod setup;
mod util;

#[cfg(test)]
mod tests;

pub use commands::{ManagerCli, ManagerCommands};
pub use dispatch::run_manager_cli;
pub use offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
