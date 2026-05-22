use std::slice;

use chia_protocol::{Bytes32, Coin};
use chia_puzzle_types::{EveProof, LineageProof, Proof, offer::NotarizedPayment, offer::Payment, singleton::SingletonArgs};
use chia_puzzles::SINGLETON_LAUNCHER_HASH;
use chia_sdk_driver::{
    Action, AssetInfo, Cat, Id, Launcher, Offer, Puzzle, Relation, RequestedPayments,
    SpendContext, Spends, StandardLayer, Vault, VaultInfo, decode_offer,
};
use chia_sdk_test::{BlsPair, R1Pair, Simulator};
use clvm_traits::ToClvm;
use clvmr::Allocator;
use indexmap::indexmap;
use sha2::{Digest, Sha256};

use crate::offer::presplit::{
    build_fixed_presplit_conditions_spend, build_offer_from_presplit_cat,
    build_presplit_conditions_inner_spend, build_presplit_split_spend_bundle_with_vault,
    vault_change_puzzle_hash,
};
use crate::vault::members::{
    MemberConfig, bls_member_hash, force_1_of_2_restriction, m_of_n_hash, prevent_vault_side_effects_restriction,
    r1_member_hash, singleton_member_hash, timelock_restriction, tree_hash_nil,
};
use crate::vault::spend::{
    VaultSpendContext, materialize_vault_cat_finished_spends_with_vault,
};

pub struct SimulatorVaultHarness {
    pub sim: Simulator,
    pub launcher_id: Bytes32,
    pub inner_puzzle_hash: clvm_utils::TreeHash,
    pub p2_message_hash: Bytes32,
    pub asset_id: Bytes32,
    pub vault_ctx: VaultSpendContext,
    pub r1: R1Pair,
}

impl SimulatorVaultHarness {
    pub fn new() -> Self {
        let r1 = R1Pair::new(42);
        let bls = BlsPair::new(1);
        let member_config = MemberConfig::default();
        let custody_hash = r1_member_hash(&member_config, r1.pk, true);
        let recovery_hash = {
            let timelock = timelock_restriction(3600);
            let member_validator_list_hash = Bytes32::from(clvm_utils::tree_hash_pair(
                timelock.puzzle_hash,
                tree_hash_nil(),
            ));
            let mut recovery_restrictions = prevent_vault_side_effects_restriction();
            recovery_restrictions.insert(
                0,
                force_1_of_2_restriction(
                    Bytes32::from(custody_hash),
                    0,
                    member_validator_list_hash,
                    Bytes32::from(tree_hash_nil()),
                ),
            );
            let recovery_config = member_config.with_restrictions(recovery_restrictions);
            bls_member_hash(&recovery_config, bls.pk, false)
        };
        let inner_puzzle_hash = m_of_n_hash(
            &member_config.with_top_level(true),
            1,
            vec![custody_hash, recovery_hash],
        );
        let vault_ctx = VaultSpendContext::new_test_context(
            Bytes32::default(),
            inner_puzzle_hash,
            custody_hash,
            recovery_hash,
            r1.pk,
        );
        Self {
            sim: Simulator::new(),
            launcher_id: Bytes32::default(),
            inner_puzzle_hash,
            p2_message_hash: Bytes32::default(),
            asset_id: Bytes32::default(),
            vault_ctx,
            r1,
        }
    }

    fn vault_singleton_puzzle_hash(&self) -> Bytes32 {
        SingletonArgs::curry_tree_hash(self.launcher_id, self.inner_puzzle_hash).into()
    }

    fn sync_launcher_binding(&mut self, launcher_id: Bytes32) {
        self.launcher_id = launcher_id;
        self.p2_message_hash = singleton_member_hash(
            &MemberConfig::default().with_top_level(true),
            launcher_id,
            false,
        )
        .into();
        self.vault_ctx.launcher_id = launcher_id;
        self.vault_ctx.seed_nonce_cache(self.p2_message_hash, 0);
    }

    pub fn mint_vault(&mut self) {
        let mut ctx = SpendContext::new();
        let funder = self.sim.bls(10_000_000_000);
        let funder_p2 = StandardLayer::new(funder.pk);
        let (mint_conditions, vault) = Launcher::new(funder.coin.coin_id(), 1)
            .mint_vault(&mut ctx, self.inner_puzzle_hash, ())
            .expect("mint vault");
        funder_p2
            .spend(&mut ctx, funder.coin, mint_conditions)
            .expect("fund launcher");
        self.sim
            .spend_coins(ctx.take(), slice::from_ref(&funder.sk))
            .expect("confirm launcher spend");
        self.sync_launcher_binding(vault.info.launcher_id);
    }

    pub fn fund_vault_cat(&mut self, cat_amount: u64) -> Cat {
        if self.sim
            .unspent_coins(self.vault_singleton_puzzle_hash(), false)
            .is_empty()
        {
            self.mint_vault();
        }
        let mut ctx = SpendContext::new();
        let issuer = self.sim.bls(10_000_000_000);
        let vault_hint = ctx.hint(self.p2_message_hash).expect("vault hint");
        let mut spends = Spends::new(issuer.puzzle_hash);
        spends.add(issuer.coin);
        let deltas = spends
            .apply(
                &mut ctx,
                &[
                    Action::single_issue_cat(None, cat_amount),
                    Action::send(Id::New(0), self.p2_message_hash, cat_amount, vault_hint),
                ],
            )
            .expect("apply cat funding");
        let outputs = spends
            .finish_with_keys(
                &mut ctx,
                &deltas,
                Relation::None,
                &indexmap! { issuer.puzzle_hash => issuer.pk },
            )
            .expect("finish cat funding");
        self.sim
            .spend_coins(ctx.take(), slice::from_ref(&issuer.sk))
            .expect("confirm cat funding");

        let cat = outputs.cats[&Id::New(0)]
            .iter()
            .find(|cat| cat.coin.amount == cat_amount && cat.info.p2_puzzle_hash == self.p2_message_hash)
            .cloned()
            .expect("funded cat coin");
        self.asset_id = cat.info.asset_id;
        cat
    }

    pub fn latest_vault(&self) -> Vault {
        fetch_vault_from_sim(&self.sim, self.launcher_id, self.inner_puzzle_hash)
            .expect("latest vault")
    }

    pub fn sign_fast_forward(&self, message: Vec<u8>) -> chia_secp::R1Signature {
        let digest: [u8; 32] = Sha256::digest(message).into();
        self.r1.sk.sign_prehashed(&digest).expect("sign")
    }
}

fn fetch_vault_from_sim(
    sim: &Simulator,
    launcher_id: Bytes32,
    inner_puzzle_hash: clvm_utils::TreeHash,
) -> Result<Vault, String> {
    let puzzle_hash = SingletonArgs::curry_tree_hash(launcher_id, inner_puzzle_hash).into();
    let coin = sim
        .unspent_coins(puzzle_hash, false)
        .into_iter()
        .next()
        .ok_or_else(|| "missing vault coin".to_string())?;
    let parent_spend = sim
        .coin_spend(coin.parent_coin_info)
        .ok_or_else(|| "missing parent spend".to_string())?;
    let mut allocator = Allocator::new();
    let parent_puzzle = parent_spend
        .puzzle_reveal
        .to_clvm(&mut allocator)
        .map_err(|err| err.to_string())?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle);
    let proof = if parent_puzzle.curried_puzzle_hash() == SINGLETON_LAUNCHER_HASH.into() {
        Proof::Eve(EveProof {
            parent_parent_coin_info: parent_spend.coin.parent_coin_info,
            parent_amount: parent_spend.coin.amount,
        })
    } else {
        Proof::Lineage(LineageProof {
            parent_parent_coin_info: parent_spend.coin.parent_coin_info,
            parent_inner_puzzle_hash: inner_puzzle_hash.into(),
            parent_amount: parent_spend.coin.amount,
        })
    };
    Ok(Vault::new(
        coin,
        proof,
        VaultInfo::new(launcher_id, inner_puzzle_hash),
    ))
}

fn fetch_cat_from_sim(sim: &Simulator, coin: Coin) -> Result<Cat, String> {
    let parent_spend = sim
        .coin_spend(coin.parent_coin_info)
        .ok_or_else(|| "missing parent spend".to_string())?;
    let mut allocator = Allocator::new();
    let parent_puzzle = parent_spend
        .puzzle_reveal
        .to_clvm(&mut allocator)
        .map_err(|err| err.to_string())?;
    let parent_puzzle = Puzzle::parse(&allocator, parent_puzzle);
    let parent_solution = parent_spend
        .solution
        .to_clvm(&mut allocator)
        .map_err(|err| err.to_string())?;
    let children = Cat::parse_children(
        &mut allocator,
        parent_spend.coin,
        parent_puzzle,
        parent_solution,
    )
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "not a cat spend".to_string())?;
    children
        .into_iter()
        .find(|cat| cat.coin.coin_id() == coin.coin_id())
        .ok_or_else(|| "cat child not found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_puzzle_types::Memos;
    use chia_sdk_types::{Condition, Conditions, run_puzzle};
    use clvm_traits::FromClvm;
    use clvmr::NodePtr;

    use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;

    #[tokio::test]
    async fn simulator_accepts_vault_mixed_split() {
        let mut harness = SimulatorVaultHarness::new();
        let cat = harness.fund_vault_cat(5_000);
        let receive_puzzle_hash = harness.p2_message_hash;
        let mut ctx = SpendContext::new();
        let mut spends = Spends::new(receive_puzzle_hash);
        spends.add(cat);
        let deltas = spends
            .apply(
                &mut ctx,
                &[Action::send(
                    Id::Existing(harness.asset_id),
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
            .sim
            .spend_coins(spend_bundle.coin_spends, &[])
            .expect("mixed split accepted");
    }

    #[tokio::test]
    async fn simulator_presplit_and_offer_roundtrip() {
        let mut harness = SimulatorVaultHarness::new();
        let source_cat = harness.fund_vault_cat(5_000);
        let mut planning_ctx = SpendContext::new();
        let mut requested_payments = RequestedPayments::new();
        requested_payments.xch.push(NotarizedPayment::new(
            Bytes32::default(),
            vec![Payment::new(harness.p2_message_hash, 1_000_000_000_000, Memos::None)],
        ));
        let asset_info = AssetInfo::new();
        let fixed_spend = build_fixed_presplit_conditions_spend(
            &mut planning_ctx,
            &requested_payments,
            &asset_info,
            1_000,
            Some(4_000_000_000),
        )
        .expect("fixed spend");
        let p2_hashes = p2_conditions_or_singleton_puzzle_hash(
            planning_ctx.tree_hash(fixed_spend.puzzle),
            harness.launcher_id,
        );
        let vault = harness.latest_vault();
        let (split_bundle, predicted) = build_presplit_split_spend_bundle_with_vault(
            &mut harness.vault_ctx.clone(),
            std::slice::from_ref(&source_cat),
            vault_change_puzzle_hash(harness.launcher_id),
            p2_hashes.puzzle_hash.into(),
            1_000,
            4_000,
            vault,
            |message| {
                let signature = harness.sign_fast_forward(message);
                async move { Ok(signature) }
            },
        )
        .await
        .expect("split bundle");
        harness
            .sim
            .spend_coins(split_bundle.coin_spends, &[])
            .expect("split accepted");

        let presplit_coin = harness
            .sim
            .unspent_coins(predicted.info.puzzle_hash().into(), false)
            .into_iter()
            .find(|coin| coin.amount == 1_000)
            .expect("presplit coin on chain");
        let presplit_cat = fetch_cat_from_sim(&harness.sim, presplit_coin).expect("presplit cat");
        assert_eq!(presplit_cat.coin.coin_id(), predicted.coin.coin_id());

        let offer_nonce = Offer::nonce(vec![presplit_cat.coin.coin_id()]);
        let requested_payments = {
            let mut payments = RequestedPayments::new();
            payments.xch.push(NotarizedPayment::new(
                offer_nonce,
                vec![Payment::new(harness.p2_message_hash, 1_000_000_000_000, Memos::None)],
            ));
            payments
        };
        let (offer_text, _, _) = build_offer_from_presplit_cat(
            presplit_cat,
            harness.launcher_id,
            requested_payments,
            asset_info,
            1_000,
            Some(4_000_000_000),
        )
        .await
        .expect("offer");
        let decoded = decode_offer(&offer_text).expect("decode offer");
        assert!(!decoded.coin_spends.is_empty());
    }

    #[test]
    fn presplit_conditions_spend_emits_fixed_conditions() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let mut ctx = SpendContext::new();
        let fixed_spend = ctx
            .delegated_spend(
                Conditions::new().create_coin(Bytes32::new([0xab; 32]), 1, Memos::None),
            )
            .expect("fixed spend");
        let inner = build_presplit_conditions_inner_spend(&mut ctx, fixed_spend, launcher_id)
            .expect("inner spend");
        let output = run_puzzle(&mut ctx, inner.puzzle, inner.solution).expect("run puzzle");
        let conditions = Conditions::<NodePtr>::from_clvm(&ctx, output).expect("conditions");
        assert!(
            conditions.iter().any(|condition| {
                matches!(condition, Condition::CreateCoin(create) if create.amount == 1)
            })
        );
    }
}
