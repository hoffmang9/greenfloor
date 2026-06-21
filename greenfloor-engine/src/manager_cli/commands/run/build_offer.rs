use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostRunOptions,
    BuildAndPostVenueOptions,
};

use super::super::clap::ManagerCommands;

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    let ManagerCommands::BuildAndPostOffer {
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
    } = command
    else {
        unreachable!("build_offer::run_command called with {command:?}");
    };

    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    let response = build_and_post_offer(BuildAndPostOfferRequest {
        program_path: ctx.program_config.clone(),
        markets_path: ctx.markets_config.clone(),
        testnet_markets_path: ctx.testnet_markets_path().map(std::path::Path::to_path_buf),
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
        test_overrides: {
            #[cfg(test)]
            {
                ctx.offer_test_overrides.clone()
            }
            #[cfg(not(test))]
            {
                crate::offer::operator::BuildOfferTestOverrides::default()
            }
        },
    })
    .await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::manager_cli::commands::clap::ManagerCommands;
    use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
    use crate::minimal_program_template::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };
    use crate::offer::operator::BuildOfferTestOverrides;

    use super::run_command;

    fn write_dry_run_program(path: &Path, home_dir: &Path) {
        write_minimal_program_with_signer(
            path,
            MinimalProgramParams {
                home_dir,
                ..Default::default()
            },
        );
    }

    #[tokio::test]
    async fn run_command_requires_market_selector() {
        let dir = tempfile::tempdir().expect("tempdir");
        let harness = ManagerContextBuilder::new(
            dir.path().join("program.yaml"),
            dir.path().join("markets.yaml"),
        )
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
        let err = run_command(
            ManagerCommands::BuildAndPostOffer {
                market_id: None,
                pair: None,
                size_base_units: 1,
                repeat: 1,
                network: "mainnet".to_string(),
                dexie_base_url: None,
                allow_take: false,
                claim_rewards: false,
                dry_run: true,
                venue: None,
                splash_base_url: None,
            },
            &harness.ctx,
        )
        .await
        .expect_err("missing market selector");
        assert!(err
            .to_string()
            .contains("provide exactly one of --market-id or --pair"));
    }

    #[tokio::test]
    async fn run_command_dry_run_emits_preview_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        write_dry_run_program(&program, dir.path());
        std::fs::write(
            &markets,
            include_str!("../../../../tests/fixtures/data/build_offer_markets.yaml"),
        )
        .expect("write markets fixture");
        let harness = ManagerContextBuilder::new(program, markets)
            .scratch_dir(dir.path().to_path_buf())
            .offer_test_overrides(BuildOfferTestOverrides {
                offer_text: Some("offer1dryrunpreviewstub".to_string()),
            })
            .build_capturing();
        let code = run_command(
            ManagerCommands::BuildAndPostOffer {
                market_id: Some("m1".to_string()),
                pair: None,
                size_base_units: 1,
                repeat: 1,
                network: "mainnet".to_string(),
                dexie_base_url: None,
                allow_take: false,
                claim_rewards: false,
                dry_run: true,
                venue: None,
                splash_base_url: None,
            },
            &harness.ctx,
        )
        .await
        .expect("build-and-post-offer");
        assert_eq!(code, 0);
        let payload = pop_json(&harness.captured);
        assert_eq!(payload.get("dry_run"), Some(&serde_json::json!(true)));
    }
}
