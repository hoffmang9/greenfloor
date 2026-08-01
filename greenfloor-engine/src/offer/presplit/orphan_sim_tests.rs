//! Simulator-backed orphan discovery / memo recovery (real CAT spends).
//!
//! Coinset HTTP is stubbed only at the transport boundary with puzzle/solution bytes
//! taken from the simulator parent spend.

use std::collections::HashSet;

use chia_protocol::{Bytes32, CoinSpend};
use chia_sdk_coinset::CoinsetClient;
use chia_sdk_driver::{Cat, CatSpend, SpendContext};
use chia_sdk_test::Simulator;
use chia_sdk_types::Conditions;
use clvm_traits::FromClvm;
use clvmr::NodePtr;

use crate::coinset::test_support::{
    mock_get_coin_record_by_name_body, mock_get_puzzle_and_solution_body,
};
use crate::hex::{normalize_hex_id, tree_hash_to_hex};
use crate::offer::lifecycle::{offers_orphan_presplit_cli, OffersOrphanPresplitCliRequest};
use crate::offer::presplit::pipeline::PresplitPaymentContext;
use crate::offer::presplit::{
    discover_orphan_presplit_candidates, offer_nonce_from_cats, recover_from_parent_spend,
    vault_change_puzzle_hash, OrphanScanRow, PresplitOfferBinding,
};
use crate::test_support::signer_config::test_signer_config;
use crate::test_support::simulator::harness::{fetch_cat_from_sim, SimulatorVaultHarness};
use crate::vault::materialize::{
    append_vault_singleton_spend_for_vault, build_vault_cat_inner_spend,
};

const OFFER_AMOUNT: u64 = 1_000;
const SOURCE_AMOUNT: u64 = 5_000;

struct CwSplitOutcome {
    parent_spend: CoinSpend,
    child: Cat,
    binding: PresplitOfferBinding,
    asset_id: String,
}

fn parent_spent_block_index(sim: &Simulator, parent_coin_id: Bytes32) -> u32 {
    sim.coin_state(parent_coin_id)
        .and_then(|state| state.spent_height)
        .unwrap_or(1)
}

/// Cloud Wallet-style split: offer `CREATE_COIN` carries `[changePH, fixedConditions]`.
#[allow(clippy::too_many_lines)]
async fn cw_style_presplit_split_on_sim(harness: &mut SimulatorVaultHarness) -> CwSplitOutcome {
    harness.mint_vault();
    let source_cat = harness.fund_vault_cat(SOURCE_AMOUNT);
    let source_coin_id = source_cat.coin.coin_id();
    let offer_nonce = offer_nonce_from_cats(std::slice::from_ref(&source_cat));
    let receive_address = crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch")
        .expect("receive address");
    let terms = crate::offer::types::OfferTerms {
        receive_address,
        offer_asset_id: hex::encode(harness.chain.asset_id),
        offer_amount: OFFER_AMOUNT,
        request_asset_id: "xch".to_string(),
        request_amount: 1,
        expires_at: None,
        bake_expiry_into_conditions: true,
    };
    let binding = PresplitOfferBinding::plan(
        harness.chain.launcher_id,
        &terms,
        harness.chain.p2_message_hash,
        offer_nonce,
    )
    .expect("binding");
    let change_puzzle_hash =
        vault_change_puzzle_hash(harness.chain.launcher_id).expect("change ph");
    let change_amount = SOURCE_AMOUNT - OFFER_AMOUNT;

    let mut ctx = SpendContext::new();
    let payment_ctx =
        PresplitPaymentContext::new(&terms, harness.chain.p2_message_hash, offer_nonce);
    let built = payment_ctx
        .build_fixed_conditions(&mut ctx, OFFER_AMOUNT, terms.conditions_expires_at())
        .expect("fixed conditions");
    assert_eq!(
        built.fixed_conditions_tree_hash,
        binding.fixed_conditions_tree_hash
    );

    // CW: createCoin(..., [changePuzzleHash, fixedConditions]) — proper two-item list.
    let (_q, conditions_ptr) = <(NodePtr, NodePtr)>::from_clvm(&ctx, built.fixed_spend.puzzle)
        .expect("peel quoted fixed conditions");
    let cw_memos = chia_puzzle_types::Memos::Some(
        ctx.alloc(&(change_puzzle_hash, (conditions_ptr, NodePtr::NIL)))
            .expect("cw memos list"),
    );
    let change_hint = ctx.hint(change_puzzle_hash).expect("change hint");
    let mut conditions =
        Conditions::new().create_coin(change_puzzle_hash, change_amount, change_hint);
    conditions = conditions.create_coin(binding.p2_puzzle_hash, OFFER_AMOUNT, cw_memos);

    let delegated = ctx.delegated_spend(conditions).expect("delegated");
    let mut vault_ctx = harness.vault_ctx.clone();
    let nonce = vault_ctx
        .infer_nonce_for_p2_hash(source_cat.info.p2_puzzle_hash)
        .expect("nonce");
    let predicted = source_cat.child(binding.p2_puzzle_hash, OFFER_AMOUNT);
    let inner_spend = build_vault_cat_inner_spend(
        &mut ctx,
        delegated,
        &vault_ctx,
        nonce,
        source_cat.info.p2_puzzle_hash.into(),
    )
    .expect("inner");
    Cat::spend_all(&mut ctx, &[CatSpend::new(source_cat, inner_spend)]).expect("cat spend");
    let vault = harness.latest_vault();
    append_vault_singleton_spend_for_vault(&mut ctx, &vault_ctx, &vault, |message| {
        let signature = harness.sign_fast_forward(message);
        async move { Ok(signature) }
    })
    .await
    .expect("singleton");

    let coin_spends = ctx.take();
    let parent_spend = coin_spends
        .iter()
        .find(|spend| spend.coin.coin_id() == source_coin_id)
        .cloned()
        .expect("source CAT spend");
    harness
        .chain
        .sim
        .lock()
        .expect("sim")
        .spend_coins(coin_spends, &[])
        .expect("accept split");

    let child_coin = harness
        .chain
        .sim
        .lock()
        .expect("sim")
        .unspent_coins(predicted.info.puzzle_hash().into(), false)
        .into_iter()
        .find(|coin| coin.amount == OFFER_AMOUNT)
        .expect("child on chain");
    let child =
        fetch_cat_from_sim(&harness.chain.sim.lock().expect("sim"), child_coin).expect("child cat");
    assert_eq!(child.coin.coin_id(), predicted.coin.coin_id());
    assert_eq!(child.info.p2_puzzle_hash, binding.p2_puzzle_hash);

    CwSplitOutcome {
        parent_spend,
        child,
        binding,
        asset_id: normalize_hex_id(&hex::encode(harness.chain.asset_id)),
    }
}

#[tokio::test]
async fn recover_from_parent_spend_reads_cw_fixed_hash_from_inner_memos() {
    let mut harness = SimulatorVaultHarness::new();
    let outcome = cw_style_presplit_split_on_sim(&mut harness).await;
    let (hash, detail) = recover_from_parent_spend(
        harness.chain.launcher_id,
        outcome.child.coin.coin_id(),
        outcome.child.coin.amount,
        &outcome.parent_spend,
    )
    .expect("recover");
    assert_eq!(detail, "parent_memo");
    let hash = hash.expect("recovered hash");
    assert_eq!(
        tree_hash_to_hex(hash),
        tree_hash_to_hex(outcome.binding.fixed_conditions_tree_hash)
    );
}

#[tokio::test]
async fn discover_orphan_candidates_recovers_cw_memo_via_coinset_parent_spend() {
    let mut harness = SimulatorVaultHarness::new();
    let outcome = cw_style_presplit_split_on_sim(&mut harness).await;
    let child = &outcome.child;
    let parent_coin = outcome.parent_spend.coin;
    let spent_block_index = {
        let sim = harness.chain.sim.lock().expect("sim");
        parent_spent_block_index(&sim, parent_coin.coin_id())
    };

    let mut server = mockito::Server::new_async().await;
    let _parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(mock_get_coin_record_by_name_body(
            &parent_coin,
            spent_block_index,
        ))
        .create_async()
        .await;
    let _puzzle_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(mock_get_puzzle_and_solution_body(&outcome.parent_spend))
        .create_async()
        .await;

    let row = OrphanScanRow {
        coin_id: hex::encode(child.coin.coin_id()),
        parent_coin_info: hex::encode(child.coin.parent_coin_info),
        amount: child.coin.amount,
        confirmed_block_index: 9_000_000,
        spent_block_index: 0,
        discovered_by_hint: true,
        discovered_by_puzzle_hash: false,
        cat_asset_id: Some(outcome.asset_id.clone()),
    };
    let client = CoinsetClient::new(server.url());
    let got = discover_orphan_presplit_candidates(
        &client,
        harness.chain.launcher_id,
        &[row],
        &outcome.asset_id,
        &HashSet::new(),
        Some(8_000_000),
    )
    .await
    .expect("discover");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].recovery_label(), "parent_memo");
    assert_eq!(
        got[0].fixed_delegated_puzzle_hash.as_deref(),
        Some(tree_hash_to_hex(outcome.binding.fixed_conditions_tree_hash).as_str())
    );
}

#[tokio::test]
async fn offers_orphan_presplit_cli_discovers_cw_orphan_without_reclaim() {
    let mut harness = SimulatorVaultHarness::new();
    let outcome = cw_style_presplit_split_on_sim(&mut harness).await;
    let child = &outcome.child;
    let parent_coin = outcome.parent_spend.coin;
    let spent_block_index = {
        let sim = harness.chain.sim.lock().expect("sim");
        parent_spent_block_index(&sim, parent_coin.coin_id())
    };

    let mut server = mockito::Server::new_async().await;
    let _parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(mock_get_coin_record_by_name_body(
            &parent_coin,
            spent_block_index,
        ))
        .create_async()
        .await;
    let _puzzle_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(mock_get_puzzle_and_solution_body(&outcome.parent_spend))
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tmp");
    let payload = offers_orphan_presplit_cli(
        test_signer_config(&server.url()),
        "mainnet",
        OffersOrphanPresplitCliRequest {
            asset_id: outcome.asset_id.clone(),
            market_id: None,
            start_height: Some(1),
            reclaim: false,
            dry_run: true,
            wait: false,
            home_dir: dir.path().to_path_buf(),
            state_db_override: None,
            launcher_id: hex::encode(harness.chain.launcher_id),
            scan_rows: vec![OrphanScanRow {
                coin_id: hex::encode(child.coin.coin_id()),
                parent_coin_info: hex::encode(child.coin.parent_coin_info),
                amount: child.coin.amount,
                confirmed_block_index: 100,
                spent_block_index: 0,
                discovered_by_hint: true,
                discovered_by_puzzle_hash: false,
                cat_asset_id: Some(outcome.asset_id),
            }],
        },
    )
    .await
    .expect("cli");

    assert_eq!(payload.discovered_count, 1);
    assert_eq!(payload.reclaimable_count, 1);
    assert_eq!(payload.orphans[0].recovery, "parent_memo");
    assert!(payload.reclaim_result.is_none());
}

#[tokio::test]
async fn offers_orphan_presplit_cli_dry_run_reclaim_reports_result() {
    let mut harness = SimulatorVaultHarness::new();
    let outcome = cw_style_presplit_split_on_sim(&mut harness).await;
    let child = &outcome.child;
    let parent_coin = outcome.parent_spend.coin;
    let spent_block_index = {
        let sim = harness.chain.sim.lock().expect("sim");
        parent_spent_block_index(&sim, parent_coin.coin_id())
    };

    let mut server = mockito::Server::new_async().await;
    // Parent spend for discovery + coin-missing responses for dry-run reclaim vault lookup.
    let _parent_mock = server
        .mock("POST", "/get_coin_record_by_name")
        .with_status(200)
        .with_body(mock_get_coin_record_by_name_body(
            &parent_coin,
            spent_block_index,
        ))
        .expect_at_least(1)
        .create_async()
        .await;
    let _puzzle_mock = server
        .mock("POST", "/get_puzzle_and_solution")
        .with_status(200)
        .with_body(mock_get_puzzle_and_solution_body(&outcome.parent_spend))
        .create_async()
        .await;

    let dir = tempfile::tempdir().expect("tmp");
    let payload = offers_orphan_presplit_cli(
        test_signer_config(&server.url()),
        "mainnet",
        OffersOrphanPresplitCliRequest {
            asset_id: outcome.asset_id.clone(),
            market_id: Some("m1".to_string()),
            start_height: None,
            reclaim: true,
            dry_run: true,
            wait: true,
            home_dir: dir.path().to_path_buf(),
            state_db_override: None,
            launcher_id: hex::encode(harness.chain.launcher_id),
            scan_rows: vec![OrphanScanRow {
                coin_id: hex::encode(child.coin.coin_id()),
                parent_coin_info: hex::encode(child.coin.parent_coin_info),
                amount: child.coin.amount,
                confirmed_block_index: 100,
                spent_block_index: 0,
                discovered_by_hint: true,
                discovered_by_puzzle_hash: false,
                cat_asset_id: Some(outcome.asset_id),
            }],
        },
    )
    .await
    .expect("cli");

    assert_eq!(payload.reclaimable_count, 1);
    let reclaim = payload.reclaim_result.expect("reclaim_result");
    assert!(reclaim.dry_run);
    assert_eq!(reclaim.selected_count, 1);
}

#[tokio::test]
async fn recover_from_gf_native_split_reports_unavailable_no_memos() {
    use crate::offer::presplit::{
        build_presplit_split_spend_bundle_with_vault, PresplitSplitParams,
    };

    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let source = harness.fund_vault_cat(SOURCE_AMOUNT);
    let offer_nonce = offer_nonce_from_cats(std::slice::from_ref(&source));
    let receive_address =
        crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch").expect("receive");
    let terms = crate::offer::types::OfferTerms {
        receive_address,
        offer_asset_id: hex::encode(harness.chain.asset_id),
        offer_amount: OFFER_AMOUNT,
        request_asset_id: "xch".to_string(),
        request_amount: 1,
        expires_at: None,
        bake_expiry_into_conditions: true,
    };
    let binding = PresplitOfferBinding::plan(
        harness.chain.launcher_id,
        &terms,
        harness.chain.p2_message_hash,
        offer_nonce,
    )
    .expect("binding");
    let source_id = source.coin.coin_id();
    let (bundle, predicted) = build_presplit_split_spend_bundle_with_vault(
        &mut harness.vault_ctx.clone(),
        std::slice::from_ref(&source),
        PresplitSplitParams {
            change_puzzle_hash: vault_change_puzzle_hash(harness.chain.launcher_id).expect("ch"),
            p2_puzzle_hash: binding.p2_puzzle_hash,
            offer_amount: OFFER_AMOUNT,
            change_amount: SOURCE_AMOUNT - OFFER_AMOUNT,
        },
        harness.latest_vault(),
        |message| {
            let signature = harness.sign_fast_forward(message);
            async move { Ok(signature) }
        },
    )
    .await
    .expect("split");
    let parent_spend = bundle
        .coin_spends
        .iter()
        .find(|spend| spend.coin.coin_id() == source_id)
        .cloned()
        .expect("parent");
    harness
        .chain
        .sim
        .lock()
        .expect("sim")
        .spend_coins(bundle.coin_spends, &[])
        .expect("accept");

    let (hash, detail) = recover_from_parent_spend(
        harness.chain.launcher_id,
        predicted.coin.coin_id(),
        OFFER_AMOUNT,
        &parent_spend,
    )
    .expect("recover");
    assert!(hash.is_none());
    assert!(detail.contains("no memos"));
}
