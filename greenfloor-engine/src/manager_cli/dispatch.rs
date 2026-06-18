//! Command dispatch for the native manager CLI.

use crate::error::SignerResult;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, OfferOperatorTestOverrides,
};

use super::commands::{ManagerCli, ManagerCommands};
use super::context::ManagerContext;
use super::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
use super::util::require_market_selector;
use super::{cats, coin_op_loop, keys, setup};
use crate::cli_util::optional_str;

pub async fn run_manager_cli(cli: ManagerCli) -> SignerResult<i32> {
    let (ctx, command) = ManagerContext::from_cli(cli);
    match command {
        ManagerCommands::ConfigValidate { program_only } => {
            setup::run_config_validate(&ctx, program_only)
        }
        ManagerCommands::ProgramFields => setup::run_program_fields(&ctx),
        ManagerCommands::MarketsFields => setup::run_markets_fields(&ctx),
        ManagerCommands::CatsFields => setup::run_cats_fields(&ctx),
        ManagerCommands::MaterializeMinimalProgram {
            output,
            home_dir,
            dexie_api_base,
            log_level,
            dry_run,
            low_inventory_alerts_enabled,
            pushover_enabled,
            with_signer,
        } => setup::run_materialize_minimal_program(setup::MaterializeMinimalProgramRequest {
            output: &output,
            home_dir: &home_dir,
            dexie_api_base: &dexie_api_base,
            log_level: &log_level,
            dry_run,
            low_inventory_alerts_enabled,
            pushover_enabled,
            with_signer,
        }),
        ManagerCommands::KeysOnboard {
            chia_keys_dir,
            key_id,
            state_dir,
        } => keys::run_keys_onboard(
            &ctx,
            &key_id,
            &state_dir,
            super::paths::optional_path(&chia_keys_dir).as_deref(),
        ),
        ManagerCommands::BuildAndPostOffer {
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
                testnet_markets_path: ctx.testnet_markets_path().map(|path| path.to_path_buf()),
                network,
                market_id,
                pair,
                size_base_units,
                repeat,
                publish_venue: venue,
                dexie_base_url: dexie_base_url.or(ctx.dexie_base_url.clone()),
                splash_base_url,
                drop_only: !allow_take,
                claim_rewards,
                dry_run,
                persist_results: true,
                action_side: None,
                test_overrides: OfferOperatorTestOverrides::from_env(),
            })
            .await?;
            ctx.emit_json(&response.payload)?;
            Ok(response.exit_code)
        }
        ManagerCommands::Doctor => setup::run_doctor(&ctx),
        ManagerCommands::OffersStatus {
            market_id,
            limit,
            events_limit,
        } => run_offers_status_command(
            &ctx,
            OffersStatusCliArgs {
                market_id,
                limit,
                events_limit,
            },
        ),
        ManagerCommands::OffersReconcile {
            market_id,
            limit,
            venue,
        } => {
            run_offers_reconcile_command(
                &ctx,
                OffersReconcileCliArgs {
                    market_id,
                    limit,
                    venue,
                },
            )
            .await
        }
        ManagerCommands::OffersCancel {
            offer_id,
            cancel_open,
            venue,
        } => {
            run_offers_cancel_command(
                &ctx,
                OffersCancelCliArgs {
                    offer_id,
                    cancel_open,
                    venue,
                },
            )
            .await
        }
        ManagerCommands::BootstrapHome {
            home_dir,
            program_template,
            markets_template,
            cats_template,
            testnet_markets_template,
            seed_testnet_markets,
            force,
        } => setup::run_bootstrap_home(setup::BootstrapHomeParams {
            ctx: &ctx,
            home_dir: &home_dir,
            program_template: &program_template,
            markets_template: &markets_template,
            cats_template: super::paths::optional_path(&cats_template).as_deref(),
            testnet_markets_template: super::paths::optional_path(&testnet_markets_template)
                .as_deref(),
            seed_testnet_markets,
            force,
        }),
        ManagerCommands::CatsAdd {
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
            cats::run_cats_add(cats::CatsAddRequest {
                ctx: &ctx,
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
        ManagerCommands::CatsList => cats::run_cats_list(&ctx).await,
        ManagerCommands::CatsDelete {
            network,
            cat_id,
            ticker,
            no_dexie_lookup,
            yes,
            preflight_only,
        } => {
            cats::run_cats_delete(
                &ctx,
                &network,
                cat_id.as_deref(),
                ticker.as_deref(),
                !no_dexie_lookup,
                yes,
                preflight_only,
            )
            .await
        }
        ManagerCommands::SetLogLevel { log_level } => setup::run_set_log_level(&ctx, &log_level),
        ManagerCommands::CoinsList {
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coins_list(
                &ctx,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
        ManagerCommands::CoinStatus {
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coin_status(
                &ctx,
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
        ManagerCommands::CoinSplit {
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
            coin_op_loop::run_coin_split(coin_op_loop::CoinSplitRequest {
                mgr: &ctx,
                network: &network,
                market_id: market_id.as_deref(),
                pair: pair.as_deref(),
                coin_ids: &coin_id,
                amount_per_coin,
                number_of_coins,
                no_wait,
                size_base_units: if size_base_units > 0 {
                    Some(size_base_units)
                } else {
                    None
                },
                until_ready,
                max_iterations,
                allow_lock_all_spendable,
                force_split_when_ready,
            })
            .await
        }
        ManagerCommands::CoinCombine {
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
                mgr: &ctx,
                network: &network,
                market_id: market_id.as_deref(),
                pair: pair.as_deref(),
                coin_ids: &coin_id,
                number_of_coins: input_coin_count,
                asset_id: optional_str(&asset_id),
                no_wait,
                size_base_units: if size_base_units > 0 {
                    Some(size_base_units)
                } else {
                    None
                },
                until_ready,
                max_iterations,
            })
            .await
        }
    }
}
