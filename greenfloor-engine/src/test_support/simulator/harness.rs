use std::slice;
use std::sync::{Arc, Mutex};

use chia_protocol::{Bytes32, Coin};
use chia_puzzle_types::{
    offer::NotarizedPayment, offer::Payment, singleton::SingletonArgs, EveProof, LineageProof,
    Memos, Proof,
};
use chia_puzzles::SINGLETON_LAUNCHER_HASH;
use chia_sdk_driver::{
    Action, Cat, Id, Launcher, Offer, Puzzle, Relation, RequestedPayments, SpendContext, Spends,
    StandardLayer, Vault, VaultInfo,
};
use chia_sdk_test::{BlsPair, R1Pair, Simulator};
use clvm_traits::ToClvm;
use clvmr::Allocator;
use indexmap::indexmap;
use sha2::{Digest, Sha256};

use crate::coinset;
use crate::vault::context::compute_vault_hashes;
use crate::vault::members::{singleton_member_hash, MemberConfig, WalletKey};
use crate::vault::spend::VaultSpendContext;

pub(crate) struct SimChain {
    pub sim: Mutex<Simulator>,
    pub launcher_id: Bytes32,
    pub inner_puzzle_hash: clvm_utils::TreeHash,
    pub p2_message_hash: Bytes32,
    pub asset_id: Bytes32,
}

pub(crate) struct SimulatorVaultHarness {
    pub chain: SimChain,
    pub vault_ctx: VaultSpendContext,
    pub r1: R1Pair,
}

impl SimulatorVaultHarness {
    pub fn new() -> Self {
        let r1 = R1Pair::new(42);
        let signer = R1Pair::new(42);
        let bls = BlsPair::new(1);
        let snapshot = test_snapshot_from_keys(&r1, &bls);
        let hashes = compute_vault_hashes(&snapshot).expect("vault hashes");
        let mut vault_ctx = VaultSpendContext::new_test_context(
            Bytes32::default(),
            hashes.inner_puzzle_hash,
            hashes.custody_hash,
            hashes.recovery_hash,
            r1.pk,
        );
        let sk = signer.sk;
        vault_ctx.set_local_fast_forward_signer(Arc::new(move |message| {
            let digest: [u8; 32] = Sha256::digest(&message).into();
            sk.sign_prehashed(&digest)
                .map_err(|err| crate::error::SignerError::Kms(err.to_string()))
        }));
        Self {
            chain: SimChain {
                sim: Mutex::new(Simulator::new()),
                launcher_id: Bytes32::default(),
                inner_puzzle_hash: hashes.inner_puzzle_hash,
                p2_message_hash: Bytes32::default(),
                asset_id: Bytes32::default(),
            },
            vault_ctx,
            r1,
        }
    }

    fn vault_singleton_puzzle_hash(&self) -> Bytes32 {
        SingletonArgs::curry_tree_hash(self.chain.launcher_id, self.chain.inner_puzzle_hash).into()
    }

    fn sync_launcher_binding(&mut self, launcher_id: Bytes32) {
        self.chain.launcher_id = launcher_id;
        self.chain.p2_message_hash = singleton_member_hash(
            &MemberConfig::default().with_top_level(true),
            launcher_id,
            false,
        )
        .expect("p2 message hash")
        .into();
        self.vault_ctx.launcher_id = launcher_id;
        self.vault_ctx
            .seed_nonce_cache(self.chain.p2_message_hash, 0);
    }

    pub fn mint_vault(&mut self) {
        let mut ctx = SpendContext::new();
        let mut sim = self.chain.sim.lock().expect("sim lock");
        let funder = sim.bls(10_000_000_000);
        let funder_p2 = StandardLayer::new(funder.pk);
        let (mint_conditions, vault) = Launcher::new(funder.coin.coin_id(), 1)
            .mint_vault(&mut ctx, self.chain.inner_puzzle_hash, ())
            .expect("mint vault");
        funder_p2
            .spend(&mut ctx, funder.coin, mint_conditions)
            .expect("fund launcher");
        sim.spend_coins(ctx.take(), slice::from_ref(&funder.sk))
            .expect("confirm launcher spend");
        drop(sim);
        self.sync_launcher_binding(vault.info.launcher_id);
    }

    pub fn fund_vault_cat(&mut self, cat_amount: u64) -> Cat {
        self.fund_vault_cat_labeled(cat_amount, 0)
    }

    /// Fund the vault with two CAT assets via sequential issuance spends.
    pub fn fund_vault_two_cats(&mut self, base_amount: u64, quote_amount: u64) -> (Cat, Cat) {
        let base_cat = self.fund_vault_cat(base_amount);
        let quote_cat = self.fund_vault_cat(quote_amount);
        assert!(
            base_cat.info.asset_id != quote_cat.info.asset_id,
            "expected distinct CAT asset ids from sequential issuance"
        );
        (base_cat, quote_cat)
    }

    /// Fund the vault with a CAT issued under ``Id::New(label)`` within one issuance spend.
    pub fn fund_vault_cat_labeled(&mut self, cat_amount: u64, label: usize) -> Cat {
        if self
            .chain
            .sim
            .lock()
            .expect("sim lock")
            .unspent_coins(self.vault_singleton_puzzle_hash(), false)
            .is_empty()
        {
            self.mint_vault();
        }
        let mut ctx = SpendContext::new();
        let mut sim = self.chain.sim.lock().expect("sim lock");
        let issuer = sim.bls(10_000_000_000);
        let vault_hint = ctx.hint(self.chain.p2_message_hash).expect("vault hint");
        let issue_id = Id::New(label);
        let mut spends = Spends::new(issuer.puzzle_hash);
        spends.add(issuer.coin);
        let deltas = spends
            .apply(
                &mut ctx,
                &[
                    Action::single_issue_cat(None, cat_amount),
                    Action::send(issue_id, self.chain.p2_message_hash, cat_amount, vault_hint),
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
        sim.spend_coins(ctx.take(), slice::from_ref(&issuer.sk))
            .expect("confirm cat funding");
        drop(sim);

        let cat = outputs.cats[&issue_id]
            .iter()
            .find(|cat| {
                cat.coin.amount == cat_amount
                    && cat.info.p2_puzzle_hash == self.chain.p2_message_hash
            })
            .copied()
            .expect("funded cat coin");
        if label == 0 {
            self.chain.asset_id = cat.info.asset_id;
        }
        cat
    }

    pub fn latest_vault(&self) -> Vault {
        fetch_vault_from_sim(
            &self.chain.sim.lock().expect("sim lock"),
            self.chain.launcher_id,
            self.chain.inner_puzzle_hash,
        )
        .expect("latest vault")
    }

    pub fn sign_fast_forward(&self, message: Vec<u8>) -> chia_secp::R1Signature {
        let digest: [u8; 32] = Sha256::digest(message).into();
        self.r1.sk.sign_prehashed(&digest).expect("sign")
    }
}

fn test_snapshot_from_keys(
    r1: &R1Pair,
    bls: &BlsPair,
) -> crate::vault::context::VaultCustodySnapshot {
    crate::vault::context::VaultCustodySnapshot {
        launcher_id: Bytes32::default(),
        custody_threshold: 1,
        recovery_threshold: 1,
        recovery_clawback_timelock: 3600,
        custody_keys: vec![WalletKey {
            public_key_hex: hex::encode(r1.pk.to_bytes()),
            curve: "SECP256R1".to_string(),
        }],
        recovery_keys: vec![WalletKey {
            public_key_hex: hex::encode(bls.pk.to_bytes()),
            curve: "BLS12_381".to_string(),
        }],
    }
}

pub(crate) fn fetch_vault_from_sim(
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

pub(crate) fn fetch_cat_from_sim(sim: &Simulator, coin: Coin) -> Result<Cat, String> {
    let parent_spend = sim
        .coin_spend(coin.parent_coin_info)
        .ok_or_else(|| "missing parent spend".to_string())?;
    let parent_coin_spend = chia_protocol::CoinSpend {
        coin: parent_spend.coin,
        puzzle_reveal: parent_spend.puzzle_reveal.clone(),
        solution: parent_spend.solution.clone(),
    };
    coinset::require_cat_from_parent_spend(coin, &parent_coin_spend).map_err(|err| err.to_string())
}

pub(crate) fn fetch_cat_from_sim_by_id(chain: &SimChain, coin_id: Bytes32) -> Result<Cat, String> {
    let sim = chain.sim.lock().expect("sim lock");
    let Some(state) = sim.coin_state(coin_id) else {
        return Err(format!("missing cat coin {coin_id}"));
    };
    if state.spent_height.is_some() {
        return Err(format!("cat coin spent {coin_id}"));
    }
    fetch_cat_from_sim(&sim, state.coin)
}

#[allow(dead_code)]
pub(crate) fn xch_requested_payments(
    offer_nonce: Bytes32,
    receive_puzzle_hash: Bytes32,
    request_amount: u64,
) -> RequestedPayments {
    let mut requested_payments = RequestedPayments::new();
    requested_payments.xch.push(NotarizedPayment::new(
        offer_nonce,
        vec![Payment::new(
            receive_puzzle_hash,
            request_amount,
            Memos::None,
        )],
    ));
    requested_payments
}

pub(crate) fn take_atomic_offer_on_sim(harness: &mut SimulatorVaultHarness, offer: &Offer) {
    let mut sim = harness.chain.sim.lock().expect("sim lock");
    let taker = sim.bls(2_000_000_000_000);
    let mut take_ctx = SpendContext::new();
    let mut take_spends = Spends::new(taker.puzzle_hash);
    take_spends.add(taker.coin);
    for cats in offer.offered_coins().cats.values() {
        for cat in cats {
            take_spends.add(*cat);
        }
    }
    let take_deltas = take_spends
        .apply(&mut take_ctx, &offer.requested_payments().actions())
        .expect("apply taker actions");
    let take_outputs = take_spends
        .finish_with_keys(
            &mut take_ctx,
            &take_deltas,
            Relation::AssertConcurrent,
            &indexmap! { taker.puzzle_hash => taker.pk },
        )
        .expect("finish taker spends");
    let take_coin_spends = take_ctx.take();
    let take_signature =
        chia_sdk_test::sign_transaction(&take_coin_spends, &[taker.sk]).expect("sign take");
    let atomic_offer = offer.clone().take(chia_protocol::SpendBundle::new(
        take_coin_spends,
        take_signature,
    ));
    sim.new_transaction(atomic_offer)
        .expect("atomic offer accepted on simulator");
    drop(sim);
    assert!(take_outputs.cats.values().flatten().next().is_some());
}

pub(crate) fn sample_create_offer_request(
    harness: &SimulatorVaultHarness,
    offer_amount: u64,
    source_cat: &Cat,
    presplit_coin_ids: Vec<Bytes32>,
    offer_coin_ids: Vec<Bytes32>,
    split_input_coins: bool,
    broadcast_split: bool,
) -> crate::offer::CreateOfferRequest {
    let receive_address = crate::bech32m::encode_address(harness.chain.p2_message_hash, "xch")
        .expect("test receive address");
    crate::offer::CreateOfferRequest {
        receive_address,
        offer_asset_id: hex::encode(harness.chain.asset_id),
        offer_amount,
        request_asset_id: "xch".to_string(),
        request_amount: 1_000_000_000_000,
        offer_coin_ids: if offer_coin_ids.is_empty() {
            vec![source_cat.coin.coin_id()]
        } else {
            offer_coin_ids
        },
        presplit_coin_ids,
        split_input_coins,
        broadcast_split,
        expires_at: None,
    }
}

#[cfg(test)]
mod coinset_parse_tests {
    use chia_protocol::CoinSpend;

    use super::SimulatorVaultHarness;
    use crate::coinset;

    /// Regression guard: CAT discovery must use ``Puzzle::parse`` + ``Cat::parse_children``
    /// (``coinset::parse_cat_from_parent_spend``), not ``Program::parse_child_cats``.
    #[tokio::test]
    async fn cat_from_parent_spend_resolves_cat_via_parse_children_on_simulator() {
        let mut harness = SimulatorVaultHarness::new();
        harness.mint_vault();
        let cat = harness.fund_vault_cat(5_000);
        let coin_id = cat.coin.coin_id();
        let sim = harness.chain.sim.lock().expect("sim lock");
        let parent = sim
            .coin_spend(cat.coin.parent_coin_info)
            .expect("parent spend");
        let parent_spend = CoinSpend {
            coin: parent.coin,
            puzzle_reveal: parent.puzzle_reveal.clone(),
            solution: parent.solution.clone(),
        };
        drop(sim);
        let resolved =
            coinset::require_cat_from_parent_spend(cat.coin, &parent_spend).expect("resolve cat");
        assert_eq!(resolved.coin.coin_id(), coin_id);
        assert_eq!(resolved.coin.amount, 5_000);
    }
}
