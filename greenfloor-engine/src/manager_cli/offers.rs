//! Manager CLI entrypoints for offers status, reconcile, and cancel.

use clap::Args;

use crate::cli_util::optional_trimmed;
use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::offer::lifecycle::{
    offers_cancel_cli, offers_status_cli, reconcile_offers_cli, OffersCancelCliResult,
};
use crate::storage::resolve_state_db_path;

use super::context::ManagerContext;

#[derive(Debug, Args)]
pub struct OffersReconcileCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersStatusCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    #[arg(long, default_value_t = 30)]
    pub events_limit: usize,
}

#[derive(Debug, Args)]
pub struct OffersCancelCliArgs {
    #[arg(long, action = clap::ArgAction::Append)]
    pub offer_id: Vec<String>,
    #[arg(long)]
    pub cancel_open: bool,
    #[arg(long)]
    pub venue: Option<String>,
}

pub async fn run_offers_reconcile_command(
    ctx: &ManagerContext,
    args: OffersReconcileCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload = reconcile_offers_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
    )
    .await?;
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

pub fn run_offers_status_command(
    ctx: &ManagerContext,
    args: &OffersStatusCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let payload = offers_status_cli(
        &db_path,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
        args.events_limit,
    )?;
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

pub async fn run_offers_cancel_command(
    ctx: &ManagerContext,
    args: OffersCancelCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, None);
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload: OffersCancelCliResult = offers_cancel_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        &args.offer_id,
        args.cancel_open,
    )
    .await?;
    let exit_code = if payload.failed_count == 0 { 0 } else { 2 };
    ctx.emit_serialized(&payload)?;
    Ok(exit_code)
}
