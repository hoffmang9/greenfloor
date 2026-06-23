use chia_protocol::Bytes32;
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Cat, Id, Relation, SpendContext, Spends};
use serde::Serialize;

pub use super::offer_roundtrip_setup::OfferRoundtripScenario;

use super::coinset_backend::SimulatorOfferCoinset;
use super::harness::SimulatorVaultHarness;
use super::offer_cancel_fixtures::TEST_XCH_MOJO_MULT;
use super::offer_roundtrip_setup::{
    build_offer_from_setup, run_offer_roundtrip, setup_roundtrip, TEST_CAT_MOJO_MULT,
};
use crate::offer::build::build_vault_cat_offer_with_spend;
use crate::offer::types::{CreateOfferRequest, OfferInput};
use crate::vault::materialize::materialize_vault_cat_finished_spends_with_vault;

/// Leg-layout scenarios exported as golden fixtures (CAT:CAT and buy-side; sell/XCH uses
/// [`OfferRoundtripScenario::Direct`]).
#[derive(Debug, Clone, Copy)]
pub enum OfferLegScenario {
    /// Buy base CAT: offer quote CAT, request base CAT (daemon buy-side leg swap).
    BuySideDirect,
    /// Sell base CAT for quote CAT (CAT:CAT pair).
    CatCatDirect,
}

impl OfferLegScenario {
    pub fn name(self) -> &'static str {
        match self {
            Self::BuySideDirect => "buy_side",
            Self::CatCatDirect => "cat_cat",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerFixtureRuntimeParity {
    pub action_side: String,
    pub resolved_base_asset_id: String,
    pub resolved_quote_asset_id: String,
    pub size_base_units: u64,
    pub quote_price: f64,
    pub base_unit_mojo_multiplier: u64,
    pub quote_unit_mojo_multiplier: u64,
}

pub struct OfferBuildExport {
    pub request: CreateOfferRequest,
    pub result: crate::offer::CreateOfferResult,
    pub runtime_parity: SignerFixtureRuntimeParity,
}

fn runtime_parity_sell(
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    offer_amount: u64,
    request_amount: u64,
    base_unit_mojo_multiplier: u64,
    quote_unit_mojo_multiplier: u64,
) -> SignerFixtureRuntimeParity {
    let size_base_units = offer_amount / base_unit_mojo_multiplier;
    let quote_price = crate::offer::pricing::u64_to_f64(request_amount)
        / (crate::offer::pricing::u64_to_f64(size_base_units)
            * crate::offer::pricing::u64_to_f64(quote_unit_mojo_multiplier));
    SignerFixtureRuntimeParity {
        action_side: "sell".to_string(),
        resolved_base_asset_id: resolved_base_asset_id.to_string(),
        resolved_quote_asset_id: resolved_quote_asset_id.to_string(),
        size_base_units,
        quote_price,
        base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier,
    }
}

fn runtime_parity_buy(
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    offer_amount: u64,
    request_amount: u64,
    base_unit_mojo_multiplier: u64,
    quote_unit_mojo_multiplier: u64,
) -> SignerFixtureRuntimeParity {
    let size_base_units = request_amount / base_unit_mojo_multiplier;
    let quote_price = crate::offer::pricing::u64_to_f64(offer_amount)
        / (crate::offer::pricing::u64_to_f64(size_base_units)
            * crate::offer::pricing::u64_to_f64(quote_unit_mojo_multiplier));
    SignerFixtureRuntimeParity {
        action_side: "buy".to_string(),
        resolved_base_asset_id: resolved_base_asset_id.to_string(),
        resolved_quote_asset_id: resolved_quote_asset_id.to_string(),
        size_base_units,
        quote_price,
        base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier,
    }
}

fn runtime_parity_for_roundtrip(
    harness: &SimulatorVaultHarness,
    request: &CreateOfferRequest,
) -> SignerFixtureRuntimeParity {
    runtime_parity_sell(
        &hex::encode(harness.chain.asset_id),
        "xch",
        request.offer_amount,
        request.request_amount,
        TEST_CAT_MOJO_MULT,
        TEST_XCH_MOJO_MULT,
    )
}

fn build_leg_request(
    harness: &SimulatorVaultHarness,
    scenario: OfferLegScenario,
    offer_cat: &Cat,
    request_cat: &Cat,
) -> CreateOfferRequest {
    let receive_address = crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch")
        .expect("test receive address");
    match scenario {
        OfferLegScenario::BuySideDirect => {
            let base_cat = offer_cat;
            let quote_cat = request_cat;
            CreateOfferRequest {
                receive_address,
                offer_asset_id: hex::encode(quote_cat.info.asset_id),
                offer_amount: quote_cat.coin.amount,
                request_asset_id: hex::encode(base_cat.info.asset_id),
                request_amount: 1_000,
                offer_coin_ids: vec![quote_cat.coin.coin_id()],
                presplit_coin_ids: vec![],
                split_input_coins: false,
                broadcast_split: false,
                expires_at: None,
            }
        }
        OfferLegScenario::CatCatDirect => {
            let base_cat = offer_cat;
            let quote_cat = request_cat;
            CreateOfferRequest {
                receive_address,
                offer_asset_id: hex::encode(base_cat.info.asset_id),
                offer_amount: base_cat.coin.amount,
                request_asset_id: hex::encode(quote_cat.info.asset_id),
                request_amount: 2_000,
                offer_coin_ids: vec![base_cat.coin.coin_id()],
                presplit_coin_ids: vec![],
                split_input_coins: false,
                broadcast_split: false,
                expires_at: None,
            }
        }
    }
}

fn runtime_parity_for_leg(
    scenario: OfferLegScenario,
    base_cat: &Cat,
    quote_cat: &Cat,
    request: &CreateOfferRequest,
) -> SignerFixtureRuntimeParity {
    let base_asset = hex::encode(base_cat.info.asset_id);
    let quote_asset = hex::encode(quote_cat.info.asset_id);
    match scenario {
        OfferLegScenario::BuySideDirect => runtime_parity_buy(
            &base_asset,
            &quote_asset,
            request.offer_amount,
            request.request_amount,
            TEST_CAT_MOJO_MULT,
            TEST_CAT_MOJO_MULT,
        ),
        OfferLegScenario::CatCatDirect => runtime_parity_sell(
            &base_asset,
            &quote_asset,
            request.offer_amount,
            request.request_amount,
            TEST_CAT_MOJO_MULT,
            TEST_CAT_MOJO_MULT,
        ),
    }
}

async fn build_leg_offer(scenario: OfferLegScenario) -> OfferBuildExport {
    let mut harness = SimulatorVaultHarness::new();
    let (base_cat, quote_cat) = harness.fund_vault_two_cats(5_000, 5_000);
    let request = build_leg_request(&harness, scenario, &base_cat, &quote_cat);
    let runtime_parity = runtime_parity_for_leg(scenario, &base_cat, &quote_cat, &request);
    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    coinset.register_cat(base_cat);
    coinset.register_cat(quote_cat);
    let input = OfferInput::try_from(request.clone()).expect("offer input");
    let result = build_vault_cat_offer_with_spend(&mut harness.vault_ctx, &coinset, input)
        .await
        .unwrap_or_else(|err| panic!("{} offer: {err}", scenario.name()));
    OfferBuildExport {
        request,
        result,
        runtime_parity,
    }
}

pub async fn export_offer_leg_fixture(scenario: OfferLegScenario) -> OfferBuildExport {
    build_leg_offer(scenario).await
}

pub async fn export_offer_fixture(scenario: OfferRoundtripScenario) -> OfferBuildExport {
    let mut setup = setup_roundtrip(scenario).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .unwrap_or_else(|err| panic!("{} offer: {err}", scenario.name()));
    let runtime_parity = runtime_parity_for_roundtrip(&setup.harness, &setup.request);
    OfferBuildExport {
        request: setup.request,
        result,
        runtime_parity,
    }
}

async fn leg_offer_builds_on_simulator(scenario: OfferLegScenario) {
    let built = build_leg_offer(scenario).await;
    assert!(built.result.offer.starts_with("offer1"));
    assert_ne!(built.request.offer_asset_id, built.request.request_asset_id);
    match scenario {
        OfferLegScenario::BuySideDirect => {
            assert!(built.request.offer_amount >= built.request.request_amount);
        }
        OfferLegScenario::CatCatDirect => {}
    }
}

#[tokio::test]
async fn offer_leg_scenarios_build_on_simulator() {
    for scenario in [
        OfferLegScenario::BuySideDirect,
        OfferLegScenario::CatCatDirect,
    ] {
        leg_offer_builds_on_simulator(scenario).await;
    }
}

#[tokio::test]
async fn build_vault_cat_offer_roundtrips() {
    let scenarios = [
        OfferRoundtripScenario::Direct,
        OfferRoundtripScenario::PresplitNew {
            broadcast_split: false,
        },
        OfferRoundtripScenario::PresplitNew {
            broadcast_split: true,
        },
        OfferRoundtripScenario::PresplitExisting,
    ];
    for scenario in scenarios {
        run_offer_roundtrip(scenario).await;
    }
}

#[tokio::test]
async fn simulator_accepts_vault_mixed_split() {
    let mut harness = SimulatorVaultHarness::new();
    let cat = harness.fund_vault_cat(5_000);
    let receive_puzzle_hash = harness.chain.p2_message_hash;
    let mut ctx = SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    spends.add(cat);
    let deltas = spends
        .apply(
            &mut ctx,
            &[Action::send(
                Id::Existing(harness.chain.asset_id),
                receive_puzzle_hash,
                1_000,
                Memos::None,
            )],
        )
        .expect("apply");
    let finished = spends
        .prepare(&mut ctx, &deltas, Relation::None)
        .expect("prepare");
    let vault = harness.latest_vault();
    let spend_bundle = materialize_vault_cat_finished_spends_with_vault(
        &mut ctx,
        &mut harness.vault_ctx.clone(),
        finished,
        vault,
        |message| {
            let signature = harness.sign_fast_forward(message);
            async move { Ok(signature) }
        },
    )
    .await
    .expect("materialize");
    harness
        .chain
        .sim
        .lock()
        .expect("sim lock")
        .spend_coins(spend_bundle.coin_spends, &[])
        .expect("mixed split accepted");
}

#[test]
fn presplit_conditions_spend_emits_fixed_conditions() {
    use chia_sdk_driver::SpendContext;
    use chia_sdk_types::{run_puzzle, Condition, Conditions};
    use clvm_traits::FromClvm;
    use clvmr::NodePtr;

    use crate::offer::presplit::build_presplit_conditions_inner_spend;

    let launcher_id = Bytes32::new([0xcc; 32]);
    let mut ctx = SpendContext::new();
    let fixed_spend = ctx
        .delegated_spend(Conditions::new().create_coin(Bytes32::new([0xab; 32]), 1, Memos::None))
        .expect("fixed spend");
    let inner = build_presplit_conditions_inner_spend(&mut ctx, fixed_spend, launcher_id)
        .expect("inner spend");
    let output = run_puzzle(&mut ctx, inner.puzzle, inner.solution).expect("run puzzle");
    let conditions = Conditions::<NodePtr>::from_clvm(&ctx, output).expect("conditions");
    assert!(conditions.iter().any(|condition| {
        matches!(condition, Condition::CreateCoin(create) if create.amount == 1)
    }));
}
