//! Simulator fixtures for offer cancel / reclaim tests.

use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::{offer::NotarizedPayment, offer::Payment, Memos};
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{Action, AssetInfo, Id, Offer, Relation, SpendContext, Spends};
use chia_sdk_test::sign_transaction;
use clvmr::Allocator;
use indexmap::indexmap;

use super::harness::SimulatorVaultHarness;

pub(crate) const TEST_XCH_MOJO_MULT: u64 = 1_000_000_000_000;

/// Build a minimal XCH offer whose maker coin uses a standard BLS p2 puzzle (non-vault).
pub(crate) fn build_standard_p2_xch_offer(
    harness: &SimulatorVaultHarness,
    offer_amount: u64,
) -> String {
    let mut sim = harness.chain.sim.lock().expect("sim lock");
    let maker = sim.bls(offer_amount.saturating_add(10));
    let offer_nonce = Bytes32::new([0xaa; 32]);
    let mut requested = chia_sdk_driver::RequestedPayments::new();
    requested.xch.push(NotarizedPayment::new(
        offer_nonce,
        vec![Payment::new(
            harness.chain.p2_message_hash,
            TEST_XCH_MOJO_MULT,
            Memos::None,
        )],
    ));

    let mut ctx = SpendContext::new();
    let mut spends = Spends::new(maker.puzzle_hash);
    spends.add(maker.coin);
    let change = maker.coin.amount.saturating_sub(offer_amount);
    let mut actions = vec![Action::send(
        Id::Xch,
        SETTLEMENT_PAYMENT_HASH.into(),
        offer_amount,
        Memos::None,
    )];
    if change > 0 {
        actions.push(Action::send(
            Id::Xch,
            maker.puzzle_hash,
            change,
            Memos::None,
        ));
    }
    let deltas = spends.apply(&mut ctx, &actions).expect("apply maker spend");
    spends
        .finish_with_keys(
            &mut ctx,
            &deltas,
            Relation::None,
            &indexmap! { maker.puzzle_hash => maker.pk },
        )
        .expect("finish maker spend");
    let coin_spends = ctx.take();
    let signature = sign_transaction(&coin_spends, &[maker.sk]).expect("sign");
    let input_bundle = SpendBundle::new(coin_spends, signature);

    let mut allocator = Allocator::new();
    let offer =
        Offer::from_input_spend_bundle(&mut allocator, input_bundle, requested, AssetInfo::new())
            .expect("offer");
    let mut offer_ctx = SpendContext::new();
    let offer_bundle = offer.to_spend_bundle(&mut offer_ctx).expect("offer bundle");
    crate::bech32m::encode_offer(&offer_bundle).expect("encode")
}
