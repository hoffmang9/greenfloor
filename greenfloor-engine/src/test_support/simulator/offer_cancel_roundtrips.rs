//! Simulator tests for on-chain offer cancel and vault reclaim spend construction.

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::SpendContext;

use super::coinset_backend::SimulatorOfferCoinset;
use super::harness::SimulatorVaultHarness;
use super::offer_cancel_fixtures::{build_standard_p2_xch_offer, TEST_XCH_MOJO_MULT};
use super::offer_roundtrip_setup::{
    build_offer_from_setup, setup_roundtrip, OfferRoundtripScenario, RoundtripSetup,
    TEST_CAT_MOJO_MULT,
};
use crate::coinset::OfferCoinsetBackend;
use crate::error::SignerError;
use crate::offer::classify_cancellable_maker_input;
use crate::offer::presplit::{
    build_offer_from_presplit_xch, vault_change_puzzle_hash, PresplitOfferBinding,
    PresplitPaymentContext,
};
use crate::offer::reclaim::{
    build_offer_cancel_spend_bundle, build_vault_cat_reclaim_spend_bundle, OfferReclaimMode,
};
use crate::offer::types::{OfferInput, StoredOfferCancelMetadata};
use crate::vault::materialize::{
    append_vault_p2_reclaim_spend, finalize_vault_reclaim_spend_bundle,
};
use crate::vault::spend::VaultFastForwardSigner;

fn stored_cancel_metadata(
    result: &crate::offer::types::CreateOfferResult,
) -> StoredOfferCancelMetadata {
    StoredOfferCancelMetadata {
        fields: result
            .presplit_cancel_fields
            .clone()
            .expect("presplit cancel fields"),
        execution_mode: Some(result.execution_mode),
    }
}

fn corrupt_presplit_maker_puzzle_reveal(spend_bundle: &mut SpendBundle, presplit_coin_id: Bytes32) {
    let mut allocator = clvmr::Allocator::new();
    let garbage = allocator
        .new_atom(b"not-presplit-puzzle")
        .expect("garbage puzzle atom");
    let puzzle_bytes = clvmr::serde::node_to_bytes(&allocator, garbage).expect("puzzle bytes");
    for coin_spend in &mut spend_bundle.coin_spends {
        if coin_spend.coin.coin_id() == presplit_coin_id {
            coin_spend.puzzle_reveal = puzzle_bytes.clone().into();
        }
    }
}

async fn spend_vault_cat_reclaim(
    setup: &mut RoundtripSetup,
    cat: chia_sdk_driver::Cat,
    mode: OfferReclaimMode,
) {
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    coinset.register_cat(cat);
    let change_puzzle_hash =
        vault_change_puzzle_hash(setup.harness.vault_ctx.launcher_id).expect("change ph");
    let vault = coinset
        .fetch_latest_vault(
            setup.harness.vault_ctx.launcher_id,
            setup.harness.vault_ctx.inner_puzzle_hash,
        )
        .await
        .expect("vault");
    let signer = VaultFastForwardSigner::from_context(&setup.harness.vault_ctx);
    let reclaim_bundle = build_vault_cat_reclaim_spend_bundle(
        &mut setup.harness.vault_ctx,
        cat,
        change_puzzle_hash,
        mode,
        &vault,
        move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        },
    )
    .await
    .expect("reclaim bundle");
    setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(reclaim_bundle.coin_spends, &[])
        .expect("reclaim accepted");
}

#[tokio::test]
async fn build_presplit_offer_reclaim_spend_returns_cat_to_vault() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let presplit_coin_id = presplit_cat.coin.coin_id();
    let spend_bundle = crate::bech32m::decode_offer(&result.offer).expect("decode offer");
    let binding = PresplitOfferBinding::from_presplit_coin_input(
        setup.harness.vault_ctx.launcher_id,
        presplit_cat.coin,
        &spend_bundle,
    )
    .expect("extract binding");
    spend_vault_cat_reclaim(
        &mut setup,
        presplit_cat,
        OfferReclaimMode::PresplitOffer {
            fixed_conditions_tree_hash: binding.fixed_conditions_tree_hash,
        },
    )
    .await;
    assert!(setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .coin_state(presplit_coin_id)
        .expect("presplit coin")
        .spent_height
        .is_some());
}

#[tokio::test]
async fn build_offer_cancel_spend_bundle_presplit_existing_returns_cat_to_vault() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let presplit_coin_id = presplit_cat.coin.coin_id();
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    coinset.register_cat(presplit_cat);
    let spend_bundle = crate::bech32m::decode_offer(&result.offer).expect("decode offer");
    let extracted = PresplitOfferBinding::from_presplit_coin_input(
        setup.harness.vault_ctx.launcher_id,
        presplit_cat.coin,
        &spend_bundle,
    )
    .expect("extract binding");
    let input = OfferInput::try_from(setup.request.clone()).expect("offer input");
    let terms = input.terms();
    let offer_nonce =
        crate::offer::presplit::offer_nonce_from_coin_ids(&setup.request.offer_coin_ids);
    let planned = PresplitOfferBinding::plan(
        setup.harness.vault_ctx.launcher_id,
        terms,
        setup.harness.chain.p2_message_hash,
        offer_nonce,
    )
    .expect("plan binding");
    assert_eq!(
        extracted.fixed_conditions_tree_hash, planned.fixed_conditions_tree_hash,
        "cancel binding must match offer build"
    );
    let cancel_bundle = build_offer_cancel_spend_bundle(
        &mut setup.harness.vault_ctx,
        &coinset,
        &result.offer,
        None,
    )
    .await
    .expect("presplit-existing cancel bundle");
    setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(cancel_bundle.coin_spends, &[])
        .expect("presplit cancel accepted");
    assert!(
        setup
            .harness
            .chain
            .sim
            .lock()
            .expect("sim lock")
            .coin_state(presplit_coin_id)
            .expect("presplit coin")
            .spent_height
            .is_some(),
        "presplit offer input must be spent by cancel"
    );
}

#[tokio::test]
async fn classify_presplit_cat_uses_stored_metadata_before_offer_binding() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let presplit_coin_id = presplit_cat.coin.coin_id();
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    coinset.register_cat(presplit_cat);
    let mut corrupt_bundle = crate::bech32m::decode_offer(&result.offer).expect("decode offer");
    corrupt_presplit_maker_puzzle_reveal(&mut corrupt_bundle, presplit_coin_id);
    let metadata = stored_cancel_metadata(&result);
    let maker_coin = presplit_cat.coin;
    classify_cancellable_maker_input(
        &mut setup.harness.vault_ctx,
        &coinset,
        &corrupt_bundle,
        None,
        maker_coin,
    )
    .await
    .expect_err("corrupt maker spend without stored metadata must not classify");
    let _input = classify_cancellable_maker_input(
        &mut setup.harness.vault_ctx,
        &coinset,
        &corrupt_bundle,
        Some(&metadata),
        maker_coin,
    )
    .await
    .expect("stored metadata must classify presplit cat without offer binding");
    let cancel_bundle = build_offer_cancel_spend_bundle(
        &mut setup.harness.vault_ctx,
        &coinset,
        &result.offer,
        Some(&metadata),
    )
    .await
    .expect("stored metadata cancel with valid offer");
    setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(cancel_bundle.coin_spends, &[])
        .expect("presplit cancel accepted");
    assert!(
        setup
            .harness
            .chain
            .sim
            .lock()
            .expect("sim lock")
            .coin_state(presplit_coin_id)
            .expect("presplit coin")
            .spent_height
            .is_some(),
        "presplit offer input must be spent by cancel"
    );
}

#[tokio::test]
async fn classify_presplit_cat_without_coinset_uses_stored_metadata() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let presplit_coin_id = presplit_cat.coin.coin_id();
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    let mut corrupt_bundle = crate::bech32m::decode_offer(&result.offer).expect("decode offer");
    corrupt_presplit_maker_puzzle_reveal(&mut corrupt_bundle, presplit_coin_id);
    let metadata = stored_cancel_metadata(&result);
    let maker_coin = presplit_cat.coin;
    classify_cancellable_maker_input(
        &mut setup.harness.vault_ctx,
        &coinset,
        &corrupt_bundle,
        None,
        maker_coin,
    )
    .await
    .expect_err("coinset miss without stored metadata must not classify");
    classify_cancellable_maker_input(
        &mut setup.harness.vault_ctx,
        &coinset,
        &corrupt_bundle,
        Some(&metadata),
        maker_coin,
    )
    .await
    .expect("stored metadata must classify presplit cat without coinset registration");
}

#[tokio::test]
async fn build_offer_cancel_presplit_cat_without_coinset_registration() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let presplit_coin_id = presplit_cat.coin.coin_id();
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    let cancel_bundle = build_offer_cancel_spend_bundle(
        &mut setup.harness.vault_ctx,
        &coinset,
        &result.offer,
        None,
    )
    .await
    .expect("presplit-existing cancel without coinset cat");
    setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(cancel_bundle.coin_spends, &[])
        .expect("presplit cancel accepted");
    assert!(setup
        .harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .coin_state(presplit_coin_id)
        .expect("presplit coin")
        .spent_height
        .is_some());
}

#[tokio::test]
async fn build_vault_cat_reclaim_spend_returns_offered_coin_to_vault() {
    let mut harness = SimulatorVaultHarness::new();
    let cat = harness.fund_vault_cat(1_000);
    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    coinset.register_cat(cat);
    let change_puzzle_hash =
        vault_change_puzzle_hash(harness.vault_ctx.launcher_id).expect("change ph");
    let vault = coinset
        .fetch_latest_vault(
            harness.vault_ctx.launcher_id,
            harness.vault_ctx.inner_puzzle_hash,
        )
        .await
        .expect("vault");
    let offered_coin_id = cat.coin.coin_id();
    let signer = VaultFastForwardSigner::from_context(&harness.vault_ctx);
    let reclaim_bundle = build_vault_cat_reclaim_spend_bundle(
        &mut harness.vault_ctx,
        cat,
        change_puzzle_hash,
        OfferReclaimMode::DirectVault,
        &vault,
        move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        },
    )
    .await
    .expect("direct reclaim bundle");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(reclaim_bundle.coin_spends, &[])
        .expect("direct reclaim accepted");
    assert!(harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .coin_state(offered_coin_id)
        .expect("offered coin")
        .spent_height
        .is_some());
}

#[tokio::test]
async fn build_vault_xch_reclaim_spend_returns_offered_coin_to_vault() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let amount = TEST_XCH_MOJO_MULT;
    let xch_coin = {
        let mut sim = harness.chain.sim.lock().expect("sim lock");
        sim.new_coin(harness.chain.p2_message_hash, amount)
    };
    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    let change_puzzle_hash =
        vault_change_puzzle_hash(harness.vault_ctx.launcher_id).expect("change ph");
    let vault = coinset
        .fetch_latest_vault(
            harness.vault_ctx.launcher_id,
            harness.vault_ctx.inner_puzzle_hash,
        )
        .await
        .expect("vault");
    let offered_coin_id = xch_coin.coin_id();
    let signer = VaultFastForwardSigner::from_context(&harness.vault_ctx);
    let mut ctx = SpendContext::new();
    append_vault_p2_reclaim_spend(
        &mut ctx,
        xch_coin,
        change_puzzle_hash,
        &harness.vault_ctx,
        harness.chain.p2_message_hash.into(),
        0,
    )
    .expect("append xch reclaim");
    let reclaim_bundle =
        finalize_vault_reclaim_spend_bundle(ctx, &mut harness.vault_ctx, &vault, move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        })
        .await
        .expect("direct xch reclaim bundle");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(reclaim_bundle.coin_spends, &[])
        .expect("direct xch reclaim accepted");
    assert!(harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .coin_state(offered_coin_id)
        .expect("offered xch coin")
        .spent_height
        .is_some());
}

#[tokio::test]
async fn build_offer_cancel_rejects_non_vault_maker_coin() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let offer_text = build_standard_p2_xch_offer(&harness, TEST_XCH_MOJO_MULT);
    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    let err = build_offer_cancel_spend_bundle(&mut harness.vault_ctx, &coinset, &offer_text, None)
        .await
        .expect_err("non-vault maker must fail");
    assert!(
        matches!(err, SignerError::OfferCancelInputNotVaultOwned { .. }),
        "expected OfferCancelInputNotVaultOwned, got {err}"
    );
}

#[tokio::test]
async fn build_offer_cancel_rejects_spent_direct_vault_cat() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::Direct).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("direct vault cat offer");
    let offered_cat = setup.source_cat;
    spend_vault_cat_reclaim(&mut setup, offered_cat, OfferReclaimMode::DirectVault).await;
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    let err = build_offer_cancel_spend_bundle(
        &mut setup.harness.vault_ctx,
        &coinset,
        &result.offer,
        None,
    )
    .await
    .expect_err("spent direct vault cat must fail fast");
    assert!(
        matches!(err, SignerError::OfferCancelInputCoinAlreadySpent),
        "expected OfferCancelInputCoinAlreadySpent, got {err}"
    );
}

#[tokio::test]
async fn build_offer_cancel_rejects_spent_presplit_cat() {
    let mut setup = setup_roundtrip(OfferRoundtripScenario::PresplitExisting).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .expect("presplit-existing offer");
    let presplit_cat = setup.presplit_cat.expect("presplit cat");
    let spend_bundle = crate::bech32m::decode_offer(&result.offer).expect("decode offer");
    let binding = PresplitOfferBinding::from_presplit_coin_input(
        setup.harness.vault_ctx.launcher_id,
        presplit_cat.coin,
        &spend_bundle,
    )
    .expect("extract binding");
    spend_vault_cat_reclaim(
        &mut setup,
        presplit_cat,
        OfferReclaimMode::PresplitOffer {
            fixed_conditions_tree_hash: binding.fixed_conditions_tree_hash,
        },
    )
    .await;
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    let err = build_offer_cancel_spend_bundle(
        &mut setup.harness.vault_ctx,
        &coinset,
        &result.offer,
        None,
    )
    .await
    .expect_err("spent presplit cat must fail fast");
    assert!(
        matches!(err, SignerError::OfferCancelInputCoinAlreadySpent),
        "expected OfferCancelInputCoinAlreadySpent, got {err}"
    );
}

#[tokio::test]
async fn classify_direct_vault_p2_rejects_spent_coin() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let amount = TEST_XCH_MOJO_MULT;
    let xch_coin = {
        let mut sim = harness.chain.sim.lock().expect("sim lock");
        sim.new_coin(harness.chain.p2_message_hash, amount)
    };
    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    let change_puzzle_hash =
        vault_change_puzzle_hash(harness.vault_ctx.launcher_id).expect("change ph");
    let vault = coinset
        .fetch_latest_vault(
            harness.vault_ctx.launcher_id,
            harness.vault_ctx.inner_puzzle_hash,
        )
        .await
        .expect("vault");
    let signer = VaultFastForwardSigner::from_context(&harness.vault_ctx);
    let mut ctx = SpendContext::new();
    append_vault_p2_reclaim_spend(
        &mut ctx,
        xch_coin,
        change_puzzle_hash,
        &harness.vault_ctx,
        harness.chain.p2_message_hash.into(),
        0,
    )
    .expect("append xch reclaim");
    let reclaim_bundle =
        finalize_vault_reclaim_spend_bundle(ctx, &mut harness.vault_ctx, &vault, move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        })
        .await
        .expect("reclaim bundle");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(reclaim_bundle.coin_spends, &[])
        .expect("reclaim accepted");
    let spend_bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
    let err = classify_cancellable_maker_input(
        &mut harness.vault_ctx,
        &coinset,
        &spend_bundle,
        None,
        xch_coin,
    )
    .await
    .expect_err("spent direct vault p2 must fail fast");
    assert!(
        matches!(err, SignerError::OfferCancelInputCoinAlreadySpent),
        "expected OfferCancelInputCoinAlreadySpent, got {err}"
    );
}

#[tokio::test]
async fn build_offer_cancel_spend_bundle_presplit_xch_returns_coin_to_vault() {
    let mut harness = SimulatorVaultHarness::new();
    harness.mint_vault();
    let _cat = harness.fund_vault_cat(TEST_CAT_MOJO_MULT);
    let offer_amount = TEST_XCH_MOJO_MULT;
    let offer_nonce = Bytes32::new([0x55; 32]);
    let receive_address = crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch")
        .expect("receive address");
    let terms = crate::offer::types::OfferTerms {
        receive_address,
        offer_asset_id: "xch".to_string(),
        offer_amount,
        request_asset_id: hex::encode(harness.chain.asset_id),
        request_amount: TEST_CAT_MOJO_MULT,
        expires_at: None,
    };
    let binding = PresplitOfferBinding::plan(
        harness.vault_ctx.launcher_id,
        &terms,
        harness.chain.p2_message_hash,
        offer_nonce,
    )
    .expect("plan presplit xch binding");
    let presplit_coin = {
        let mut sim = harness.chain.sim.lock().expect("sim lock");
        sim.new_coin(binding.p2_puzzle_hash, offer_amount)
    };
    let payment_ctx =
        PresplitPaymentContext::new(&terms, harness.chain.p2_message_hash, offer_nonce);
    let (offer_text, _, _) = build_offer_from_presplit_xch(
        presplit_coin,
        harness.vault_ctx.launcher_id,
        &binding,
        &payment_ctx,
    )
    .expect("presplit xch offer");
    let presplit_coin_id = presplit_coin.coin_id();
    let cancel_bundle = build_offer_cancel_spend_bundle(
        &mut harness.vault_ctx,
        &SimulatorOfferCoinset::new(&harness.chain),
        &offer_text,
        None,
    )
    .await
    .expect("presplit xch cancel bundle");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(cancel_bundle.coin_spends, &[])
        .expect("presplit xch cancel accepted");
    assert!(
        harness
            .chain
            .sim
            .lock()
            .expect("sim lock")
            .coin_state(presplit_coin_id)
            .expect("presplit xch coin")
            .spent_height
            .is_some(),
        "presplit xch offer input must be spent by cancel"
    );
}
