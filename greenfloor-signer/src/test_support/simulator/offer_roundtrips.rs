use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Id, Offer, Relation, SpendContext, Spends};
use chia_traits::Streamable;

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

#[derive(Debug, Clone, Copy)]
enum OfferRoundtripScenario {
    Direct,
    PresplitNew { broadcast_split: bool },
    PresplitExisting,
}

impl OfferRoundtripScenario {
    fn name(self) -> &'static str {
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

async fn split_presplit_cat_on_sim(
    harness: &mut SimulatorVaultHarness,
    source_cat: chia_sdk_driver::Cat,
    offer_amount: u64,
) -> (chia_sdk_driver::Cat, Bytes32) {
    let source_coin_id = source_cat.coin.coin_id();
    let offer_nonce = offer_nonce_from_cats(std::slice::from_ref(&source_cat));
    let requested_payments = xch_requested_payments(
        offer_nonce,
        harness.chain.p2_message_hash,
        1_000_000_000_000,
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
    source_cat: &chia_sdk_driver::Cat,
    presplit_cat: Option<&chia_sdk_driver::Cat>,
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

async fn run_offer_roundtrip(scenario: OfferRoundtripScenario) {
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

    let coinset = SimulatorOfferCoinset::new(&harness.chain);
    if let Some(presplit_cat) = presplit_cat {
        coinset.register_cat(presplit_cat);
    } else {
        coinset.register_cat(source_cat);
    }

    let request = build_request(
        &harness,
        scenario,
        offer_amount,
        &source_cat,
        presplit_cat.as_ref(),
        source_coin_id,
    );
    let input = OfferInput::try_from(request).expect("offer input");

    let result = build_vault_cat_offer_with_spend(&mut harness.vault_ctx, &coinset, input)
        .await
        .unwrap_or_else(|err| panic!("{} offer: {err}", scenario.name()));

    match scenario {
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
                harness
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
            let expected = presplit_cat.map(|cat| hex::encode(cat.coin.coin_id()));
            assert_eq!(result.presplit_coin_id, expected);
        }
    }

    take_atomic_offer_on_sim(&mut harness, &offer_from_result(&result));
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
