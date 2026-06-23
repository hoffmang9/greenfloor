//! Shared simulator setup for vault CAT offer roundtrip and cancel tests.

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{Cat, Offer, SpendContext};
use chia_traits::Streamable;

use super::coinset_backend::SimulatorOfferCoinset;
use super::harness::{fetch_cat_from_sim, sample_create_offer_request, SimulatorVaultHarness};
use super::offer_cancel_fixtures::TEST_XCH_MOJO_MULT;
use crate::offer::build::build_vault_cat_offer_with_spend;
use crate::offer::presplit::{
    build_presplit_split_spend_bundle_with_vault, offer_nonce_from_cats, vault_change_puzzle_hash,
    PresplitOfferBinding, PresplitSplitParams,
};
use crate::offer::types::{CreateOfferRequest, OfferExecutionMode, OfferInput};

pub(crate) const TEST_CAT_MOJO_MULT: u64 = 1_000;

#[derive(Debug, Clone, Copy)]
pub enum OfferRoundtripScenario {
    Direct,
    PresplitNew { broadcast_split: bool },
    PresplitExisting,
}

impl OfferRoundtripScenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::PresplitNew {
                broadcast_split: false,
            } => "presplit_new",
            Self::PresplitNew {
                broadcast_split: true,
            } => "presplit_new_broadcast",
            Self::PresplitExisting => "presplit_existing",
        }
    }
}

pub(crate) struct RoundtripSetup {
    pub harness: SimulatorVaultHarness,
    pub source_cat: Cat,
    pub request: CreateOfferRequest,
    pub presplit_cat: Option<Cat>,
    pub scenario: OfferRoundtripScenario,
}

async fn split_presplit_cat_on_sim(
    harness: &mut SimulatorVaultHarness,
    source_cat: Cat,
    offer_amount: u64,
) -> (Cat, Bytes32) {
    let source_coin_id = source_cat.coin.coin_id();
    let offer_nonce = offer_nonce_from_cats(std::slice::from_ref(&source_cat));
    let receive_address = crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch")
        .expect("test receive address");
    let terms = crate::offer::types::OfferTerms {
        receive_address,
        offer_asset_id: hex::encode(harness.chain.asset_id),
        offer_amount,
        request_asset_id: "xch".to_string(),
        request_amount: TEST_XCH_MOJO_MULT,
        expires_at: None,
    };
    let binding = PresplitOfferBinding::plan(
        harness.chain.launcher_id,
        &terms,
        harness.chain.p2_message_hash,
        offer_nonce,
    )
    .expect("binding");
    let change_amount = source_cat.coin.amount - offer_amount;
    let vault = harness.latest_vault();
    let (split_bundle, predicted) = build_presplit_split_spend_bundle_with_vault(
        &mut harness.vault_ctx.clone(),
        std::slice::from_ref(&source_cat),
        PresplitSplitParams {
            change_puzzle_hash: vault_change_puzzle_hash(harness.chain.launcher_id)
                .expect("change"),
            p2_puzzle_hash: binding.p2_puzzle_hash,
            offer_amount,
            change_amount,
        },
        vault,
        |message| {
            let signature = harness.sign_fast_forward(message);
            async move { Ok(signature) }
        },
    )
    .await
    .expect("split bundle");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(split_bundle.coin_spends, &[])
        .expect("split accepted");
    let presplit_coin = harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .unspent_coins(predicted.info.puzzle_hash().into(), false)
        .into_iter()
        .find(|coin| coin.amount == offer_amount)
        .expect("presplit coin on chain");
    let presplit_cat =
        fetch_cat_from_sim(&harness.chain.sim.lock().expect("sim lock"), presplit_coin)
            .expect("presplit cat");
    assert_eq!(presplit_cat.coin.coin_id(), predicted.coin.coin_id());
    assert_eq!(binding.p2_puzzle_hash, presplit_cat.info.p2_puzzle_hash);
    (presplit_cat, source_coin_id)
}

fn build_request(
    harness: &SimulatorVaultHarness,
    scenario: OfferRoundtripScenario,
    offer_amount: u64,
    source_cat: &Cat,
    presplit_cat: Option<&Cat>,
    source_coin_id: Option<Bytes32>,
) -> CreateOfferRequest {
    match scenario {
        OfferRoundtripScenario::Direct => sample_create_offer_request(
            harness,
            offer_amount,
            source_cat,
            vec![],
            vec![],
            false,
            false,
        ),
        OfferRoundtripScenario::PresplitNew { broadcast_split } => sample_create_offer_request(
            harness,
            offer_amount,
            source_cat,
            vec![],
            vec![],
            true,
            broadcast_split,
        ),
        OfferRoundtripScenario::PresplitExisting => sample_create_offer_request(
            harness,
            offer_amount,
            source_cat,
            vec![presplit_cat.expect("presplit cat").coin.coin_id()],
            vec![source_coin_id.expect("source coin id")],
            false,
            false,
        ),
    }
}

pub(crate) async fn setup_roundtrip(scenario: OfferRoundtripScenario) -> RoundtripSetup {
    let mut harness = SimulatorVaultHarness::new();
    let offer_amount = 1_000;
    let source_cat = match scenario {
        OfferRoundtripScenario::Direct => harness.fund_vault_cat(offer_amount),
        _ => harness.fund_vault_cat(5_000),
    };

    let (presplit_cat, source_coin_id) =
        if matches!(scenario, OfferRoundtripScenario::PresplitExisting) {
            let (presplit_cat, source_coin_id) =
                split_presplit_cat_on_sim(&mut harness, source_cat, offer_amount).await;
            (Some(presplit_cat), Some(source_coin_id))
        } else {
            (None, None)
        };

    let request = build_request(
        &harness,
        scenario,
        offer_amount,
        &source_cat,
        presplit_cat.as_ref(),
        source_coin_id,
    );

    RoundtripSetup {
        harness,
        source_cat,
        request,
        presplit_cat,
        scenario,
    }
}

pub(crate) async fn build_offer_from_setup(
    setup: &mut RoundtripSetup,
) -> Result<crate::offer::CreateOfferResult, crate::error::SignerError> {
    let input = OfferInput::try_from(setup.request.clone()).expect("offer input");
    let coinset = SimulatorOfferCoinset::new(&setup.harness.chain);
    if let Some(presplit_cat) = setup.presplit_cat {
        coinset.register_cat(presplit_cat);
    } else {
        coinset.register_cat(setup.source_cat);
    }
    build_vault_cat_offer_with_spend(&mut setup.harness.vault_ctx, &coinset, input).await
}

pub(crate) fn assert_roundtrip_result(
    setup: &mut RoundtripSetup,
    result: &crate::offer::CreateOfferResult,
) {
    match setup.scenario {
        OfferRoundtripScenario::Direct => {
            assert_eq!(result.execution_mode, OfferExecutionMode::Direct);
            assert_eq!(result.selected_coin_ids.len(), 1);
            assert!(result.presplit_coin_id.is_none());
        }
        OfferRoundtripScenario::PresplitNew { broadcast_split } => {
            assert_eq!(result.execution_mode, OfferExecutionMode::PresplitNew);
            assert_eq!(result.selected_coin_ids.len(), 1);
            assert!(result.presplit_coin_id.is_some());
            assert!(result.split_spend_bundle_hex.is_some());
            if broadcast_split {
                assert_eq!(result.split_broadcast_status.as_deref(), Some("SUCCESS"));
            } else {
                assert!(result.split_broadcast_status.is_none());
                let split_hex = result
                    .split_spend_bundle_hex
                    .as_ref()
                    .expect("split bundle hex");
                let split_bytes =
                    hex::decode(split_hex.trim_start_matches("0x")).expect("split hex");
                let split_bundle = SpendBundle::from_bytes(&split_bytes).expect("split bundle");
                setup
                    .harness
                    .chain
                    .sim
                    .lock()
                    .expect("sim lock")
                    .spend_coins(split_bundle.coin_spends, &[])
                    .expect("manual split broadcast");
            }
        }
        OfferRoundtripScenario::PresplitExisting => {
            assert_eq!(result.execution_mode, OfferExecutionMode::PresplitExisting);
            assert!(result.selected_coin_ids.is_empty());
            let expected = setup
                .presplit_cat
                .map(|cat| hex::encode(cat.coin.coin_id()));
            assert_eq!(result.presplit_coin_id, expected);
        }
    }
}

pub(crate) fn offer_from_result(result: &crate::offer::CreateOfferResult) -> Offer {
    let offer_bytes =
        hex::decode(result.spend_bundle_hex.trim_start_matches("0x")).expect("offer hex");
    let offer_bundle = SpendBundle::from_bytes(&offer_bytes).expect("offer bundle bytes");
    let mut offer_ctx = SpendContext::new();
    Offer::from_spend_bundle(&mut offer_ctx, &offer_bundle).expect("valid offer structure")
}

pub(crate) async fn run_offer_roundtrip(scenario: OfferRoundtripScenario) {
    let mut setup = setup_roundtrip(scenario).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .unwrap_or_else(|err| panic!("{} offer: {err}", scenario.name()));
    assert_roundtrip_result(&mut setup, &result);
    super::harness::take_atomic_offer_on_sim(&mut setup.harness, &offer_from_result(&result));
}
