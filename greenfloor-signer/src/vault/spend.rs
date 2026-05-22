use std::collections::HashMap;

use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{
    Action, Cat, CatSpend, DriverError, Id, InnerPuzzleSpend, MipsSpend, Relation, Spend,
    SpendContext, Spends, Vault, mips_puzzle_hash,
};
use chia_sdk_types::{
    Condition, Conditions, Mod, run_puzzle,
    conditions::SendMessage,
    puzzles::{
        R1MemberPuzzleAssert, R1MemberPuzzleAssertSolution, SingletonMember,
        SingletonMemberSolution,
    },
};
use chia_secp::{R1PublicKey, R1Signature};
use clvm_traits::FromClvm;
use clvm_utils::TreeHash;
use clvmr::{Allocator, NodePtr, serde::node_from_bytes};

use crate::coinset::{self, MIN_CAT_OUTPUT_MOJOS};
use chia_sdk_coinset::CoinsetClient;
use crate::config::CloudWalletConfig;
use crate::error::{SignerError, SignerResult};
use crate::kms;
use crate::vault::context::{VaultCustodySnapshot, compute_vault_context, compute_vault_hashes};
use crate::vault::members::{MemberConfig, hex_to_bytes, singleton_member_hash};

#[derive(Debug, Clone)]
pub struct VaultSpendContext {
    pub launcher_id: Bytes32,
    pub inner_puzzle_hash: TreeHash,
    pub custody_hash: TreeHash,
    pub recovery_hash: TreeHash,
    pub kms_key_id: String,
    pub kms_region: String,
    pub secp256r1_public_key: R1PublicKey,
    pub max_nonce_probe: u32,
    pub network: String,
    nonce_by_p2_hash: HashMap<Bytes32, u32>,
}

impl VaultSpendContext {
    pub fn infer_nonce_for_p2_hash(&mut self, p2_puzzle_hash: Bytes32) -> Option<u32> {
        if let Some(cached) = self.nonce_by_p2_hash.get(&p2_puzzle_hash) {
            return Some(*cached);
        }
        for nonce in 0..=self.max_nonce_probe {
            let candidate = singleton_member_hash(
                &MemberConfig::default().with_top_level(true).with_nonce(nonce),
                self.launcher_id,
                false,
            );
            if Bytes32::from(candidate) == p2_puzzle_hash {
                self.nonce_by_p2_hash.insert(p2_puzzle_hash, nonce);
                return Some(nonce);
            }
        }
        None
    }

    #[cfg(test)]
    pub fn seed_nonce_cache(&mut self, p2_puzzle_hash: Bytes32, nonce: u32) {
        self.nonce_by_p2_hash.insert(p2_puzzle_hash, nonce);
    }

    #[cfg(test)]
    pub fn new_test_context(
        launcher_id: Bytes32,
        inner_puzzle_hash: TreeHash,
        custody_hash: TreeHash,
        recovery_hash: TreeHash,
        secp256r1_public_key: R1PublicKey,
    ) -> Self {
        Self {
            launcher_id,
            inner_puzzle_hash,
            custody_hash,
            recovery_hash,
            kms_key_id: "test-kms".to_string(),
            kms_region: "us-west-2".to_string(),
            secp256r1_public_key,
            max_nonce_probe: 2048,
            network: "mainnet".to_string(),
            nonce_by_p2_hash: HashMap::new(),
        }
    }
}

pub async fn resolve_vault_spend_context(
    config: CloudWalletConfig,
) -> SignerResult<VaultSpendContext> {
    let kms_public_key_hex = match config.kms_public_key_hex.clone() {
        Some(value) => value,
        None => kms::get_public_key_compressed_hex(&config.kms_key_id, &config.kms_region).await?,
    };
    let client = crate::cloud_wallet::CloudWalletClient::new(config.clone())?;
    let snapshot = client.get_vault_custody_snapshot().await?;
    build_vault_spend_context(&snapshot, &kms_public_key_hex, &config)
}

pub fn build_vault_spend_context(
    snapshot: &VaultCustodySnapshot,
    kms_public_key_hex: &str,
    config: &CloudWalletConfig,
) -> SignerResult<VaultSpendContext> {
    let display = compute_vault_context(snapshot, kms_public_key_hex, &config.network)?;
    let hashes = compute_vault_hashes(snapshot)?;
    let secp_keys = display.secp256r1_custody_keys;
    let key_bytes = hex_to_bytes(&secp_keys[0])?;
    let mut key_array = [0u8; 33];
    key_array.copy_from_slice(&key_bytes);
    let secp256r1_public_key = R1PublicKey::from_bytes(&key_array).map_err(|err| {
        SignerError::UnsupportedVaultCurve(format!("SECP256R1 decode: {err}"))
    })?;
    Ok(VaultSpendContext {
        launcher_id: snapshot.launcher_id,
        inner_puzzle_hash: hashes.inner_puzzle_hash,
        custody_hash: hashes.custody_hash,
        recovery_hash: hashes.recovery_hash,
        kms_key_id: config.kms_key_id.clone(),
        kms_region: config.kms_region.clone(),
        secp256r1_public_key,
        max_nonce_probe: 2048,
        network: config.network.clone(),
        nonce_by_p2_hash: HashMap::from([(
            hashes.p2_singleton_message_hash.into(),
            0,
        )]),
    })
}

#[derive(Debug, Clone)]
pub struct MixedSplitRequest {
    pub receive_address: String,
    pub asset_id: Bytes32,
    pub output_amounts: Vec<u64>,
    pub coin_ids: Vec<Bytes32>,
    pub allow_sub_cat_output: bool,
    pub fee_mojos: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MixedSplitResult {
    pub spend_bundle_hex: String,
    pub broadcast_status: Option<String>,
    pub selected_coin_ids: Vec<String>,
    pub offered_total: u64,
    pub target_total: u64,
    pub change_amount: u64,
}

pub(crate) fn validate_mixed_split_request(request: &MixedSplitRequest) -> SignerResult<()> {
    if request.fee_mojos > 0 {
        return Err(SignerError::MixedSplitVaultWithFeeNotSupported);
    }
    if request.output_amounts.is_empty() {
        return Err(SignerError::MissingOutputAmounts);
    }
    if request.output_amounts.iter().any(|amount| *amount == 0) {
        return Err(SignerError::InvalidOutputAmount);
    }
    if !request.allow_sub_cat_output
        && request
            .output_amounts
            .iter()
            .any(|amount| *amount < MIN_CAT_OUTPUT_MOJOS)
    {
        return Err(SignerError::CatOutputBelowMinimum);
    }
    Ok(())
}

pub async fn build_and_optionally_broadcast_vault_cat_mixed_split(
    config: CloudWalletConfig,
    request: MixedSplitRequest,
    broadcast: bool,
) -> SignerResult<MixedSplitResult> {
    validate_mixed_split_request(&request)?;

    let mut vault_ctx = resolve_vault_spend_context(config.clone()).await?;
    let coinset = coinset::client_for_network(&vault_ctx.network)?;
    let receive_puzzle_hash = chia_sdk_utils::Address::decode(&request.receive_address)
        .map_err(|err| SignerError::Other(format!("invalid receive address: {err}")))?
        .puzzle_hash;

    let cats = if request.coin_ids.is_empty() {
        coinset::list_unspent_cats(&coinset, &request.receive_address, request.asset_id).await?
    } else {
        coinset::list_unspent_cats_by_ids(&coinset, &request.coin_ids).await?
    };
    if cats.is_empty() {
        return Err(SignerError::NoUnspentCatCoins);
    }

    let target_total: u64 = request.output_amounts.iter().sum();
    let selected = if request.coin_ids.is_empty() {
        coinset::select_cats_smallest_first(cats, target_total)
    } else {
        cats
    };
    if selected.is_empty() {
        return Err(SignerError::InsufficientCatCoins);
    }
    let offered_total: u64 = selected.iter().map(|cat| cat.coin.amount).sum();
    if offered_total < target_total {
        return Err(SignerError::InsufficientCatCoins);
    }
    let change_amount = offered_total - target_total;
    if !request.allow_sub_cat_output
        && change_amount > 0
        && change_amount < MIN_CAT_OUTPUT_MOJOS
    {
        return Err(SignerError::CatChangeBelowMinimum);
    }

    let spend_bundle = build_vault_cat_mixed_split_spend_bundle(
        &mut vault_ctx,
        &coinset,
        selected.clone(),
        receive_puzzle_hash,
        request.asset_id,
        &request.output_amounts,
        change_amount,
    )
    .await?;

    let spend_bundle_hex = coinset::spend_bundle_hex(&spend_bundle)?;
    let broadcast_status = if broadcast {
        Some(coinset::broadcast_spend_bundle(&coinset, spend_bundle).await?)
    } else {
        None
    };

    Ok(MixedSplitResult {
        spend_bundle_hex,
        broadcast_status,
        selected_coin_ids: selected
            .iter()
            .map(|cat| hex::encode(cat.coin.coin_id()))
            .collect(),
        offered_total,
        target_total,
        change_amount,
    })
}

async fn build_vault_cat_mixed_split_spend_bundle(
    vault_ctx: &mut VaultSpendContext,
    coinset: &CoinsetClient,
    selected_cats: Vec<Cat>,
    receive_puzzle_hash: Bytes32,
    asset_id: Bytes32,
    output_amounts: &[u64],
    change_amount: u64,
) -> SignerResult<SpendBundle> {
    let mut ctx = SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    for cat in &selected_cats {
        spends.add(*cat);
    }

    let asset_id = Id::Existing(asset_id);
    let mut actions = Vec::new();
    for amount in output_amounts {
        actions.push(Action::send(
            asset_id,
            receive_puzzle_hash,
            *amount,
            Memos::None,
        ));
    }
    if change_amount > 0 {
        actions.push(Action::send(
            asset_id,
            receive_puzzle_hash,
            change_amount,
            Memos::None,
        ));
    }

    let deltas = spends.apply(&mut ctx, &actions).map_err(driver_err)?;
    let finished = spends
        .prepare(&mut ctx, &deltas, Relation::None)
        .map_err(driver_err)?;

    materialize_vault_cat_finished_spends(&mut ctx, vault_ctx, coinset, finished).await
}

pub async fn materialize_vault_cat_finished_spends(
    ctx: &mut SpendContext,
    vault_ctx: &mut VaultSpendContext,
    coinset: &CoinsetClient,
    finished: chia_sdk_driver::Spends<chia_sdk_driver::Finished>,
) -> SignerResult<SpendBundle> {
    let vault = coinset::fetch_latest_vault(
        coinset,
        vault_ctx.launcher_id,
        vault_ctx.inner_puzzle_hash,
    )
    .await?;
    let kms_key_id = vault_ctx.kms_key_id.clone();
    let kms_region = vault_ctx.kms_region.clone();
    materialize_vault_cat_finished_spends_with_vault(ctx, vault_ctx, finished, vault, |message| {
        Box::pin(async move {
            sign_vault_fast_forward_digest(&kms_key_id, &kms_region, message).await
        })
    })
    .await
}

pub(crate) async fn materialize_vault_cat_finished_spends_with_vault<F, Fut>(
    ctx: &mut SpendContext,
    vault_ctx: &mut VaultSpendContext,
    finished: chia_sdk_driver::Spends<chia_sdk_driver::Finished>,
    vault: Vault,
    sign_digest: F,
) -> SignerResult<SpendBundle>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<R1Signature>>,
{
    let mut cat_spends = Vec::new();
    for (asset, kind) in finished.unspent() {
        let chia_sdk_driver::SpendableAsset::Cat(cat) = asset else {
            continue;
        };
        let chia_sdk_driver::SpendKind::Conditions(spend) = kind else {
            return Err(SignerError::Driver(
                "unexpected settlement spend in vault cat spend".to_string(),
            ));
        };
        let delegated = ctx
            .delegated_spend(spend.finish())
            .map_err(driver_err)?;
        let nonce = vault_ctx.infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash).ok_or(
            SignerError::Driver(
                "failed to infer vault nonce for cat p2 puzzle hash".to_string(),
            ),
        )?;
        let inner_spend = build_vault_cat_inner_spend(
            ctx,
            delegated,
            vault_ctx,
            nonce,
            cat.info.p2_puzzle_hash.into(),
        )?;
        cat_spends.push(CatSpend::new(cat, inner_spend));
    }
    if cat_spends.is_empty() {
        return Err(SignerError::Driver(
            "no cat spends produced for vault transaction".to_string(),
        ));
    }
    Cat::spend_all(ctx, &cat_spends).map_err(driver_err)?;
    append_vault_singleton_spend_for_vault(ctx, vault_ctx, &vault, sign_digest).await?;
    Ok(SpendBundle::new(ctx.take(), chia_bls::Signature::default()))
}

pub(crate) fn build_vault_cat_inner_spend(
    ctx: &mut SpendContext,
    delegated: Spend,
    vault_ctx: &VaultSpendContext,
    nonce: u32,
    p2_puzzle_hash: TreeHash,
) -> SignerResult<Spend> {
    let mut mips_spend = MipsSpend::new(delegated);
    let restrictions = Vec::new();
    let member = SingletonMember::new(vault_ctx.launcher_id);
    let member_hash = mips_puzzle_hash(
        nonce as usize,
        restrictions.clone(),
        member.curry_tree_hash(),
        true,
    );
    let member_puzzle = ctx.curry(member).map_err(driver_err)?;
    let member_solution = ctx
        .alloc(&SingletonMemberSolution::new(
            vault_ctx.inner_puzzle_hash.into(),
            1,
        ))
        .map_err(driver_err)?;
    mips_spend.members.insert(
        member_hash,
        InnerPuzzleSpend::new(
            nonce as usize,
            restrictions,
            Spend::new(member_puzzle, member_solution),
        ),
    );
    mips_spend.spend(ctx, p2_puzzle_hash).map_err(driver_err)
}

pub(crate) async fn sign_vault_fast_forward_digest(
    kms_key_id: &str,
    kms_region: &str,
    signature_message: Vec<u8>,
) -> SignerResult<R1Signature> {
    let signature_hex =
        kms::sign_digest(kms_key_id, kms_region, &hex::encode(signature_message)).await?;
    let signature_bytes = hex::decode(kms::normalize_hex(&signature_hex))
        .map_err(|err| SignerError::Kms(format!("invalid signature hex: {err}")))?;
    let signature_array: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| SignerError::Kms("invalid compact signature length".to_string()))?;
    R1Signature::from_bytes(&signature_array)
        .map_err(|err| SignerError::Kms(format!("invalid r1 signature: {err}")))
}

pub(crate) async fn append_vault_singleton_spend(
    ctx: &mut SpendContext,
    vault_ctx: &VaultSpendContext,
    coinset: &CoinsetClient,
) -> SignerResult<()> {
    let vault = coinset::fetch_latest_vault(
        coinset,
        vault_ctx.launcher_id,
        vault_ctx.inner_puzzle_hash,
    )
    .await?;
    let kms_key_id = vault_ctx.kms_key_id.clone();
    let kms_region = vault_ctx.kms_region.clone();
    append_vault_singleton_spend_for_vault(ctx, vault_ctx, &vault, |message| {
        Box::pin(async move {
            sign_vault_fast_forward_digest(&kms_key_id, &kms_region, message).await
        })
    })
    .await
}

pub(crate) async fn append_vault_singleton_spend_for_vault<F, Fut>(
    ctx: &mut SpendContext,
    vault_ctx: &VaultSpendContext,
    vault: &Vault,
    sign_digest: F,
) -> SignerResult<()>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<R1Signature>>,
{
    let receive_messages = extract_mode23_receive_messages(ctx)?;
    if receive_messages.is_empty() {
        return Err(SignerError::VaultReceiveMessageNotFound);
    }
    let mut conditions = Conditions::new().create_coin(
        vault_ctx.inner_puzzle_hash.into(),
        vault.coin.amount,
        Memos::None,
    );
    for (message, coin_id) in receive_messages {
        let coin_ptr = ctx.alloc(&coin_id).map_err(driver_err)?;
        conditions = conditions.with(SendMessage::new(23, message.into(), vec![coin_ptr]));
    }
    let delegated_spend = ctx.delegated_spend(conditions).map_err(driver_err)?;
    let delegated_hash = ctx.tree_hash(delegated_spend.puzzle);
    let signature_message = [delegated_hash.to_bytes(), vault.coin.puzzle_hash.to_bytes()].concat();
    let signature = sign_digest(signature_message).await?;

    let mut mips_spend = MipsSpend::new(delegated_spend);
    mips_spend.members.insert(
        vault_ctx.inner_puzzle_hash,
        InnerPuzzleSpend::m_of_n(
            0,
            Vec::new(),
            1,
            vec![vault_ctx.custody_hash, vault_ctx.recovery_hash],
        ),
    );

    let member = R1MemberPuzzleAssert::new(vault_ctx.secp256r1_public_key);
    let member_puzzle = ctx.curry(member).map_err(driver_err)?;
    let member_solution = ctx
        .alloc(&R1MemberPuzzleAssertSolution::new(
            vault.coin.puzzle_hash,
            signature,
        ))
        .map_err(driver_err)?;
    mips_spend.members.insert(
        vault_ctx.custody_hash,
        InnerPuzzleSpend::new(0, Vec::new(), Spend::new(member_puzzle, member_solution)),
    );

    vault.spend(ctx, &mips_spend).map_err(driver_err)?;
    Ok(())
}

fn extract_mode23_receive_messages(
    ctx: &SpendContext,
) -> SignerResult<Vec<(Vec<u8>, Bytes32)>> {
    let mut messages = Vec::new();
    for coin_spend in ctx.iter() {
        let mut allocator = Allocator::new();
        let puzzle = node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let solution = node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let output = run_puzzle(&mut allocator, puzzle, solution)
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
            .map_err(|err| SignerError::Driver(err.to_string()))?;
        for condition in conditions.iter() {
            if let Condition::ReceiveMessage(receive) = condition {
                if receive.mode == 23 {
                    messages.push((receive.message.to_vec(), coin_spend.coin.coin_id()));
                }
            }
        }
    }
    Ok(messages)
}

fn driver_err(err: DriverError) -> SignerError {
    SignerError::Driver(err.to_string())
}

impl From<DriverError> for SignerError {
    fn from(err: DriverError) -> Self {
        driver_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::Bytes32;
    use crate::vault::members::MemberConfig;
    use chia_sdk_test::R1Pair;

    fn sample_request(output_amounts: Vec<u64>, allow_sub_cat_output: bool) -> MixedSplitRequest {
        MixedSplitRequest {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w".to_string(),
            asset_id: Bytes32::default(),
            output_amounts,
            coin_ids: vec![Bytes32::default(), Bytes32::new([0xbb; 32])],
            allow_sub_cat_output,
            fee_mojos: 0,
        }
    }

    #[test]
    fn rejects_sub_unit_cat_outputs() {
        let err = validate_mixed_split_request(&sample_request(vec![999], false)).unwrap_err();
        assert!(matches!(err, SignerError::CatOutputBelowMinimum));
    }

    #[test]
    fn allow_sub_cat_output_bypasses_floor_guard() {
        validate_mixed_split_request(&sample_request(vec![999], true)).expect("allowed");
    }

    #[test]
    fn rejects_vault_mixed_split_with_fee() {
        let mut request = sample_request(vec![1000], false);
        request.fee_mojos = 1;
        let err = validate_mixed_split_request(&request).unwrap_err();
        assert!(matches!(err, SignerError::MixedSplitVaultWithFeeNotSupported));
    }

    #[test]
    fn infer_vault_nonce_for_p2_hash_matches_nonzero_nonce() {
        let launcher_id = Bytes32::new([0x11; 32]);
        let r1 = R1Pair::new(99);
        let mut vault_ctx = VaultSpendContext {
            launcher_id,
            inner_puzzle_hash: clvm_utils::TreeHash::from(launcher_id),
            custody_hash: clvm_utils::TreeHash::from(Bytes32::new([0x22; 32])),
            recovery_hash: clvm_utils::TreeHash::from(Bytes32::new([0x33; 32])),
            kms_key_id: String::new(),
            kms_region: String::new(),
            secp256r1_public_key: r1.pk,
            max_nonce_probe: 20,
            network: "mainnet".to_string(),
            nonce_by_p2_hash: HashMap::new(),
        };
        let target = crate::vault::members::singleton_member_hash(
            &MemberConfig::default().with_top_level(true).with_nonce(7),
            launcher_id,
            false,
        );
        let inferred = vault_ctx
            .infer_nonce_for_p2_hash(target.into())
            .expect("nonce");
        assert_eq!(inferred, 7);
    }
}
