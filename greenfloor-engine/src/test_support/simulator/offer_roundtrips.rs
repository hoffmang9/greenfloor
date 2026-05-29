use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Cat, Id, Offer, Relation, SpendContext, Spends};
use chia_traits::Streamable;
use serde::Serialize;

use super::coinset_backend::SimulatorOfferCoinset;
use super::harness::{
    fetch_cat_from_sim, sample_create_offer_request, take_atomic_offer_on_sim,
    xch_requested_payments, SimulatorVaultHarness,
};
use crate::offer::build::build_vault_cat_offer_with_spend;
use crate::offer::presplit::{
    build_presplit_split_spend_bundle_with_vault, offer_nonce_from_cats, vault_change_puzzle_hash,
    PresplitSplitParams,
};
use crate::offer::types::{CreateOfferRequest, OfferExecutionMode, OfferInput};
use crate::vault::materialize::materialize_vault_cat_finished_spends_with_vault;

const TEST_CAT_MOJO_MULT: u64 = 1_000;
const TEST_XCH_MOJO_MULT: u64 = 1_000_000_000_000;

#[derive(Debug, Clone, Copy)]
pub enum OfferRoundtripScenario {
    Direct,
    PresplitNew { broadcast_split: bool },
    PresplitExisting,
}

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

struct RoundtripSetup {
    harness: SimulatorVaultHarness,
    source_cat: Cat,
    request: CreateOfferRequest,
    presplit_cat: Option<Cat>,
    scenario: OfferRoundtripScenario,
}

async fn split_presplit_cat_on_sim(
    harness: &mut SimulatorVaultHarness,
    source_cat: Cat,
    offer_amount: u64,
) -> (Cat, Bytes32) {
    let source_coin_id = source_cat.coin.coin_id();
    let offer_nonce = offer_nonce_from_cats(std::slice::from_ref(&source_cat));
    let requested_payments = xch_requested_payments(
        offer_nonce,
        harness.chain.p2_message_hash,
        TEST_XCH_MOJO_MULT,
    );
    let binding = crate::offer::presplit::PresplitOfferBinding::plan(
        harness.chain.launcher_id,
        requested_payments,
        chia_sdk_driver::AssetInfo::new(),
        offer_amount,
        None,
    )
    .expect("binding");
    let change_amount = source_cat.coin.amount - offer_amount;
    let vault = harness.latest_vault();
    let (split_bundle, predicted) = build_presplit_split_spend_bundle_with_vault(
        &mut harness.vault_ctx.clone(),
        std::slice::from_ref(&source_cat),
        PresplitSplitParams {
            change_puzzle_hash: vault_change_puzzle_hash(harness.chain.launcher_id),
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

fn offer_from_result(result: &crate::offer::CreateOfferResult) -> Offer {
    let offer_bytes =
        hex::decode(result.spend_bundle_hex.trim_start_matches("0x")).expect("offer hex");
    let offer_bundle = SpendBundle::from_bytes(&offer_bytes).expect("offer bundle bytes");
    let mut offer_ctx = SpendContext::new();
    Offer::from_spend_bundle(&mut offer_ctx, &offer_bundle).expect("valid offer structure")
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

fn runtime_parity_sell(
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    offer_amount: u64,
    request_amount: u64,
    base_unit_mojo_multiplier: u64,
    quote_unit_mojo_multiplier: u64,
) -> SignerFixtureRuntimeParity {
    let size_base_units = offer_amount / base_unit_mojo_multiplier;
    let quote_price =
        request_amount as f64 / (size_base_units as f64 * quote_unit_mojo_multiplier as f64);
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
    let quote_price =
        offer_amount as f64 / (size_base_units as f64 * quote_unit_mojo_multiplier as f64);
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

async fn setup_roundtrip(scenario: OfferRoundtripScenario) -> RoundtripSetup {
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

async fn build_offer_from_setup(
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

fn assert_roundtrip_result(setup: &mut RoundtripSetup, result: &crate::offer::CreateOfferResult) {
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

async fn run_offer_roundtrip(scenario: OfferRoundtripScenario) {
    let mut setup = setup_roundtrip(scenario).await;
    let result = build_offer_from_setup(&mut setup)
        .await
        .unwrap_or_else(|err| panic!("{} offer: {err}", scenario.name()));
    assert_roundtrip_result(&mut setup, &result);
    take_atomic_offer_on_sim(&mut setup.harness, &offer_from_result(&result));
}

fn build_leg_request(
    harness: &SimulatorVaultHarness,
    scenario: OfferLegScenario,
    offer_cat: &Cat,
    request_cat: &Cat,
) -> CreateOfferRequest {
    let receive_address =
        chia_sdk_utils::Address::new(harness.chain.p2_message_hash, "xch".to_string())
            .encode()
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
