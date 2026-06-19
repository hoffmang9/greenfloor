use crate::async_boundary::ManagerCommandFuture;
use crate::cli_util::optional_str;
use crate::coinset::CoinSpentVerifyConfig;
use crate::error::SignerResult;
use crate::manager_cli::cats::{self, CatsAddRequest};
use crate::manager_cli::coin_op_loop::{
    self, CoinSplitBehavior, CoinSplitGating, CoinSplitRequest, UntilReadyWaitMode,
};
use crate::manager_cli::combine_market_cat_dust::{
    self, CombineExecutionFlags, CombineMarketCatDustRequest,
};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::flag_groups;
use crate::manager_cli::keys;
use crate::manager_cli::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
use crate::manager_cli::paths;
use crate::manager_cli::setup::{
    run_bootstrap_home, run_cats_fields, run_config_validate, run_doctor, run_markets_fields,
    run_materialize_minimal_program, run_program_fields, run_set_log_level, BootstrapHomeParams,
    MaterializeMinimalProgramFeatureFlags, MaterializeMinimalProgramRequest,
};
use crate::manager_cli::util::require_market_selector;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostRunOptions,
    BuildAndPostVenueOptions, OfferOperatorTestOverrides,
};

use super::clap::ManagerCommands;

#[must_use]
fn optional_positive_size(size_base_units: i64) -> Option<i64> {
    (size_base_units > 0).then_some(size_base_units)
}

#[must_use]
fn until_ready_wait(until_ready: bool, no_wait: bool) -> UntilReadyWaitMode {
    UntilReadyWaitMode {
        until_ready,
        no_wait,
    }
}

impl ManagerCommands {
    /// Run this CLI command to completion.
    #[must_use]
    pub fn run(self, ctx: &ManagerContext) -> ManagerCommandFuture<'_> {
        Box::pin(self.run_async(ctx))
    }

    #[allow(clippy::too_many_lines)]
    async fn run_async(self, ctx: &ManagerContext) -> SignerResult<i32> {
        match self {
            Self::ConfigValidate { program_only } => run_config_validate(ctx, program_only),
            Self::ProgramFields => run_program_fields(ctx),
            Self::MarketsFields => run_markets_fields(ctx),
            Self::CatsFields => run_cats_fields(ctx),
            Self::MaterializeMinimalProgram {
                output,
                home_dir,
                dexie_api_base,
                log_level,
                dry_run,
                low_inventory_alerts_enabled,
                pushover_enabled,
                with_signer,
            } => Ok(run_materialize_minimal_program(
                MaterializeMinimalProgramRequest {
                    output: &output,
                    home_dir: &home_dir,
                    dexie_api_base: &dexie_api_base,
                    log_level: &log_level,
                    features: MaterializeMinimalProgramFeatureFlags {
                        dry_run,
                        low_inventory_alerts_enabled,
                        pushover_enabled,
                    },
                    with_signer,
                },
            )),
            Self::KeysOnboard {
                chia_keys_dir,
                key_id,
                state_dir,
            } => keys::run_keys_onboard(
                ctx,
                &key_id,
                &state_dir,
                paths::optional_path(&chia_keys_dir).as_deref(),
            ),
            Self::Doctor => run_doctor(ctx),
            Self::BootstrapHome {
                home_dir,
                program_template,
                markets_template,
                cats_template,
                testnet_markets_template,
                seed_testnet_markets,
                force,
            } => run_bootstrap_home(&BootstrapHomeParams {
                ctx,
                home_dir: &home_dir,
                program_template: &program_template,
                markets_template: &markets_template,
                cats_template: paths::optional_path(&cats_template).as_deref(),
                testnet_markets_template: paths::optional_path(&testnet_markets_template)
                    .as_deref(),
                seed_testnet_markets,
                force,
            }),
            Self::SetLogLevel { log_level } => run_set_log_level(ctx, &log_level),
            Self::BuildAndPostOffer {
                market_id,
                pair,
                size_base_units,
                repeat,
                network,
                dexie_base_url,
                allow_take,
                claim_rewards,
                dry_run,
                venue,
                splash_base_url,
            } => {
                require_market_selector(market_id.as_deref(), pair.as_deref())?;
                let response = build_and_post_offer(BuildAndPostOfferRequest {
                    program_path: ctx.program_config.clone(),
                    markets_path: ctx.markets_config.clone(),
                    testnet_markets_path: ctx
                        .testnet_markets_path()
                        .map(std::path::Path::to_path_buf),
                    network,
                    market_id,
                    pair,
                    size_base_units,
                    repeat,
                    publish_venue: venue,
                    dexie_base_url: dexie_base_url.or(ctx.dexie_base_url.clone()),
                    splash_base_url,
                    venue: BuildAndPostVenueOptions {
                        drop_only: !allow_take,
                        claim_rewards,
                    },
                    run: BuildAndPostRunOptions {
                        dry_run,
                        persist_results: true,
                    },
                    action_side: None,
                    test_overrides: OfferOperatorTestOverrides::from_env(),
                })
                .await?;
                ctx.emit_json(&response.payload)?;
                Ok(response.exit_code)
            }
            Self::OffersStatus {
                market_id,
                limit,
                events_limit,
            } => run_offers_status_command(
                ctx,
                &OffersStatusCliArgs {
                    market_id,
                    limit,
                    events_limit,
                },
            ),
            Self::OffersReconcile {
                market_id,
                limit,
                venue,
            } => {
                run_offers_reconcile_command(
                    ctx,
                    OffersReconcileCliArgs {
                        market_id,
                        limit,
                        venue,
                    },
                )
                .await
            }
            Self::OffersCancel {
                offer_id,
                cancel_open,
                venue,
            } => {
                run_offers_cancel_command(
                    ctx,
                    OffersCancelCliArgs {
                        offer_id,
                        cancel_open,
                        venue,
                    },
                )
                .await
            }
            Self::CatsAdd {
                network,
                cat_id,
                ticker,
                name,
                base_symbol,
                ticker_id,
                pool_id,
                last_price_xch,
                target_usd_per_unit,
                no_dexie_lookup,
                replace,
            } => {
                cats::run_cats_add(CatsAddRequest {
                    ctx,
                    network: &network,
                    cat_id: cat_id.as_deref(),
                    ticker: ticker.as_deref(),
                    name: name.as_deref(),
                    base_symbol: base_symbol.as_deref(),
                    ticker_id: ticker_id.as_deref(),
                    pool_id: pool_id.as_deref(),
                    last_price_xch: last_price_xch.as_deref(),
                    target_usd_per_unit: target_usd_per_unit.as_deref(),
                    use_dexie_lookup: !no_dexie_lookup,
                    replace,
                })
                .await
            }
            Self::CatsList => cats::run_cats_list(ctx),
            Self::CatsDelete {
                network,
                cat_id,
                ticker,
                no_dexie_lookup,
                yes,
                preflight_only,
            } => {
                cats::run_cats_delete(
                    ctx,
                    &network,
                    cat_id.as_deref(),
                    ticker.as_deref(),
                    !no_dexie_lookup,
                    yes,
                    preflight_only,
                )
                .await
            }
            Self::CoinsList {
                asset,
                vault_id,
                cat_id,
            } => {
                coin_op_loop::run_coins_list(
                    ctx,
                    optional_str(&asset),
                    optional_str(&vault_id),
                    optional_str(&cat_id),
                )
                .await
            }
            Self::CoinStatus {
                asset,
                vault_id,
                cat_id,
            } => {
                coin_op_loop::run_coin_status(
                    ctx,
                    optional_str(&asset),
                    optional_str(&vault_id),
                    optional_str(&cat_id),
                )
                .await
            }
            Self::CoinSplit {
                market_id,
                pair,
                network,
                coin_id,
                amount_per_coin,
                number_of_coins,
                size_base_units,
                until_ready,
                max_iterations,
                no_wait,
                allow_lock_all_spendable,
                force_split_when_ready,
            } => {
                require_market_selector(market_id.as_deref(), pair.as_deref())?;
                coin_op_loop::run_coin_split(CoinSplitRequest {
                    mgr: ctx,
                    network: &network,
                    market_id: market_id.as_deref(),
                    pair: pair.as_deref(),
                    coin_ids: &coin_id,
                    amount_per_coin,
                    number_of_coins,
                    behavior: CoinSplitBehavior {
                        wait: until_ready_wait(until_ready, no_wait),
                        gating: CoinSplitGating {
                            allow_lock_all_spendable,
                            force_split_when_ready,
                        },
                    },
                    size_base_units: optional_positive_size(size_base_units),
                    max_iterations,
                })
                .await
            }
            Self::CoinCombine {
                market_id,
                pair,
                network,
                input_coin_count,
                asset_id,
                coin_id,
                size_base_units,
                until_ready,
                max_iterations,
                no_wait,
            } => {
                require_market_selector(market_id.as_deref(), pair.as_deref())?;
                coin_op_loop::run_coin_combine(coin_op_loop::CoinCombineRequest {
                    mgr: ctx,
                    network: &network,
                    market_id: market_id.as_deref(),
                    pair: pair.as_deref(),
                    coin_ids: &coin_id,
                    number_of_coins: input_coin_count,
                    asset_id: optional_str(&asset_id),
                    wait: until_ready_wait(until_ready, no_wait),
                    size_base_units: optional_positive_size(size_base_units),
                    max_iterations,
                })
                .await
            }
            Self::CombineMarketCatDust {
                network,
                coinset_base_url,
                launcher_id,
                launcher_id_file,
                dust_threshold_mojos,
                max_input_coins,
                max_nonce,
                cat_asset_id,
                dry_run,
                list_only,
                verify_timeout_seconds,
                verify_poll_seconds,
            } => {
                combine_market_cat_dust::run_combine_market_cat_dust(CombineMarketCatDustRequest {
                    mgr: ctx,
                    network: optional_str(&network),
                    coinset_base_url: optional_str(&coinset_base_url),
                    launcher_id: optional_str(&launcher_id),
                    launcher_id_file: optional_str(&launcher_id_file),
                    dust_threshold_mojos,
                    max_input_coins,
                    max_nonce,
                    cat_asset_id: optional_str(&cat_asset_id),
                    verify: CoinSpentVerifyConfig {
                        timeout_seconds: verify_timeout_seconds,
                        poll_seconds: verify_poll_seconds,
                    },
                    execution: CombineExecutionFlags::from_flags(list_only, dry_run),
                })
                .await
            }
            Self::FlagGroups { subcommand } => {
                let payload = flag_groups::emit_flag_groups(&subcommand)?;
                ctx.emit_json(&payload)?;
                Ok(0)
            }
        }
    }
}
