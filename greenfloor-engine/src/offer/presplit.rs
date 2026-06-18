use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{
    encode_offer, AssetInfo, Cat, CatSpend, InnerPuzzleSpend, MipsSpend, Offer, RequestedPayments,
    Spend, SpendContext, Vault,
};
use chia_sdk_types::{conditions::AssertBeforeSecondsAbsolute, Conditions};
use clvm_utils::TreeHash;
use clvmr::{Allocator, NodePtr};

use crate::coinset::{spend_bundle_hex, OfferCoinsetBackend};
use crate::error::{SignerError, SignerResult};
use crate::vault::materialize::build_vault_cat_inner_spend;
use crate::vault::members::{
    custom_member_hash, m_of_n_hash, p2_conditions_or_singleton_puzzle_hash, singleton_member_hash,
    MemberConfig,
};
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

pub fn offer_nonce_from_cats(cats: &[Cat]) -> Bytes32 {
    Offer::nonce(cats.iter().map(|cat| cat.coin.coin_id()).collect())
}

pub fn offer_nonce_from_coin_ids(coin_ids: &[Bytes32]) -> Bytes32 {
    Offer::nonce(coin_ids.to_vec())
}

pub fn build_fixed_presplit_conditions_spend(
    ctx: &mut SpendContext,
    requested_payments: &RequestedPayments,
    asset_info: &AssetInfo,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<Spend> {
    let assertions = requested_payments
        .assertions(ctx, asset_info)
        .map_err(SignerError::from)?;
    let mut conditions = Conditions::new();
    for assertion in assertions {
        conditions = conditions.with(assertion);
    }
    if let Some(seconds) = expires_at {
        conditions = conditions.with(AssertBeforeSecondsAbsolute::new(seconds));
    }
    let settlement_memos = ctx
        .hint(SETTLEMENT_PAYMENT_HASH.into())
        .map_err(SignerError::from)?;
    conditions = conditions.create_coin(
        SETTLEMENT_PAYMENT_HASH.into(),
        offer_amount,
        settlement_memos,
    );
    ctx.delegated_spend(conditions).map_err(SignerError::from)
}

pub fn build_presplit_conditions_inner_spend(
    ctx: &mut SpendContext,
    fixed_spend: Spend,
    launcher_id: Bytes32,
) -> SignerResult<Spend> {
    let member_config = MemberConfig::default();
    let fixed_conditions_hash =
        custom_member_hash(&member_config, ctx.tree_hash(fixed_spend.puzzle));
    let p2_singleton_hash = singleton_member_hash(&member_config, launcher_id, false);
    let full_puzzle_hash = m_of_n_hash(
        &member_config.with_top_level(true),
        1,
        vec![fixed_conditions_hash, p2_singleton_hash],
    )?;

    let nil_spend = Spend::new(NodePtr::NIL, NodePtr::NIL);
    let mut mips_spend = MipsSpend::new(nil_spend);
    mips_spend.members.insert(
        fixed_conditions_hash,
        InnerPuzzleSpend::new(0, Vec::new(), fixed_spend),
    );
    mips_spend.members.insert(
        full_puzzle_hash,
        InnerPuzzleSpend::m_of_n(
            0,
            Vec::new(),
            1,
            vec![fixed_conditions_hash, p2_singleton_hash],
        ),
    );
    mips_spend
        .spend(ctx, full_puzzle_hash)
        .map_err(SignerError::from)
}

pub fn predict_presplit_cat(source_cat: &Cat, p2_puzzle_hash: Bytes32, offer_amount: u64) -> Cat {
    source_cat.child(p2_puzzle_hash, offer_amount)
}

pub fn vault_change_puzzle_hash(launcher_id: Bytes32) -> Bytes32 {
    singleton_member_hash(
        &MemberConfig::default().with_top_level(true),
        launcher_id,
        false,
    )
    .into()
}

#[derive(Debug, Clone)]
pub struct PresplitOfferBinding {
    pub requested_payments: RequestedPayments,
    pub requested_asset_info: AssetInfo,
    pub offer_amount: u64,
    pub expires_at: Option<u64>,
    pub fixed_conditions_tree_hash: TreeHash,
    pub p2_puzzle_hash: Bytes32,
}

impl PresplitOfferBinding {
    pub fn plan(
        launcher_id: Bytes32,
        requested_payments: RequestedPayments,
        requested_asset_info: AssetInfo,
        offer_amount: u64,
        expires_at: Option<u64>,
    ) -> SignerResult<Self> {
        let mut ctx = SpendContext::new();
        let fixed_spend = build_fixed_presplit_conditions_spend(
            &mut ctx,
            &requested_payments,
            &requested_asset_info,
            offer_amount,
            expires_at,
        )?;
        let fixed_conditions_tree_hash = ctx.tree_hash(fixed_spend.puzzle);
        let p2_hashes =
            p2_conditions_or_singleton_puzzle_hash(fixed_conditions_tree_hash, launcher_id)?;
        Ok(Self {
            requested_payments,
            requested_asset_info,
            offer_amount,
            expires_at,
            fixed_conditions_tree_hash,
            p2_puzzle_hash: p2_hashes.puzzle_hash.into(),
        })
    }
}

pub fn verify_presplit_cat_offer_binding(
    presplit_cat: &Cat,
    binding: &PresplitOfferBinding,
) -> SignerResult<()> {
    if presplit_cat.info.p2_puzzle_hash != binding.p2_puzzle_hash {
        return Err(SignerError::PresplitCoinPuzzleHashMismatch);
    }
    Ok(())
}

pub async fn build_presplit_split_spend_bundle<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    coinset: &C,
    source_cats: &[Cat],
    change_puzzle_hash: Bytes32,
    p2_puzzle_hash: Bytes32,
    offer_amount: u64,
    change_amount: u64,
) -> SignerResult<(SpendBundle, Cat)> {
    let vault = coinset
        .fetch_latest_vault(vault_ctx.launcher_id, vault_ctx.inner_puzzle_hash)
        .await?;
    let signer = VaultFastForwardSigner::from_context(vault_ctx);
    build_presplit_split_spend_bundle_with_vault(
        vault_ctx,
        source_cats,
        PresplitSplitParams {
            change_puzzle_hash,
            p2_puzzle_hash,
            offer_amount,
            change_amount,
        },
        vault,
        move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        },
    )
    .await
}

pub fn validate_presplit_source_cats(source_cat_count: usize) -> SignerResult<()> {
    if source_cat_count != 1 {
        return Err(SignerError::PresplitRequiresSingleSourceCat);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PresplitSplitParams {
    pub change_puzzle_hash: Bytes32,
    pub p2_puzzle_hash: Bytes32,
    pub offer_amount: u64,
    pub change_amount: u64,
}

pub(crate) async fn build_presplit_split_spend_bundle_with_vault<F, Fut>(
    vault_ctx: &mut VaultSpendContext,
    source_cats: &[Cat],
    params: PresplitSplitParams,
    vault: Vault,
    sign_digest: F,
) -> SignerResult<(SpendBundle, Cat)>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<chia_secp::R1Signature>>,
{
    validate_presplit_source_cats(source_cats.len())?;
    let source_cat = source_cats[0];
    let presplit_cat =
        predict_presplit_cat(&source_cat, params.p2_puzzle_hash, params.offer_amount);

    let mut ctx = SpendContext::new();
    let mut conditions = Conditions::new();
    if params.change_amount > 0 {
        let change_memos = ctx
            .hint(params.change_puzzle_hash)
            .map_err(SignerError::from)?;
        conditions = conditions.create_coin(
            params.change_puzzle_hash,
            params.change_amount,
            change_memos,
        );
    }
    conditions = conditions.create_coin(params.p2_puzzle_hash, params.offer_amount, Memos::None);

    let delegated = ctx.delegated_spend(conditions).map_err(SignerError::from)?;
    let nonce = vault_ctx
        .infer_nonce_for_p2_hash(source_cat.info.p2_puzzle_hash)
        .ok_or(SignerError::Driver(
            "failed to infer vault nonce for cat p2 puzzle hash".to_string(),
        ))?;
    let inner_spend = build_vault_cat_inner_spend(
        &mut ctx,
        delegated,
        vault_ctx,
        nonce,
        source_cat.info.p2_puzzle_hash.into(),
    )?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(source_cat, inner_spend)])
        .map_err(SignerError::from)?;
    crate::vault::materialize::append_vault_singleton_spend_for_vault(
        &mut ctx,
        vault_ctx,
        &vault,
        sign_digest,
    )
    .await?;
    Ok((
        SpendBundle::new(ctx.take(), chia_bls::Signature::default()),
        presplit_cat,
    ))
}

pub async fn build_offer_from_presplit_cat(
    presplit_cat: Cat,
    launcher_id: Bytes32,
    binding: PresplitOfferBinding,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    // Rebuild fixed conditions in a fresh SpendContext for the actual CAT spend.
    // PresplitOfferBinding::plan already canonicalizes p2_puzzle_hash; this ctx is
    // only for materializing the inner puzzle spend on chain.
    let mut ctx = SpendContext::new();
    let fixed_spend = build_fixed_presplit_conditions_spend(
        &mut ctx,
        &binding.requested_payments,
        &binding.requested_asset_info,
        binding.offer_amount,
        binding.expires_at,
    )?;
    let rebuilt_hash = ctx.tree_hash(fixed_spend.puzzle);
    if rebuilt_hash != binding.fixed_conditions_tree_hash {
        return Err(SignerError::Driver(
            "presplit fixed conditions hash mismatch".to_string(),
        ));
    }
    let inner_spend = build_presplit_conditions_inner_spend(&mut ctx, fixed_spend, launcher_id)?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(presplit_cat, inner_spend)])
        .map_err(SignerError::from)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());

    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle.clone(),
        binding.requested_payments,
        binding.requested_asset_info,
    )
    .map_err(SignerError::from)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(SignerError::from)?;
    let offer_text = encode_offer(&offer_spend_bundle).map_err(SignerError::from)?;
    let spend_bundle_hex = spend_bundle_hex(&offer_spend_bundle)?;
    Ok((offer_text, spend_bundle_hex, hex::encode(offer_nonce)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::members::p2_conditions_or_singleton_puzzle_hash;
    use chia_sdk_driver::{AssetInfo, RequestedPayments};
    use chia_sdk_types::run_puzzle;
    use chia_sdk_types::Condition;
    use clvm_traits::FromClvm;

    #[test]
    fn p2_conditions_or_singleton_hash_is_deterministic() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let mut ctx = SpendContext::new();
        let fixed_spend = ctx
            .delegated_spend(Conditions::new().create_coin(
                Bytes32::new([0xab; 32]),
                1,
                Memos::None,
            ))
            .expect("fixed spend");
        let puzzle_hash = ctx.tree_hash(fixed_spend.puzzle);
        let hashes =
            p2_conditions_or_singleton_puzzle_hash(puzzle_hash, launcher_id).expect("p2 hashes");
        assert_ne!(hashes.puzzle_hash.to_bytes(), [0u8; 32]);
        assert_ne!(hashes.fixed_conditions_hash.to_bytes(), [0u8; 32]);
    }

    #[test]
    fn presplit_requires_single_source_cat() {
        let err = validate_presplit_source_cats(2).unwrap_err();
        assert!(matches!(err, SignerError::PresplitRequiresSingleSourceCat));
    }

    #[test]
    fn verify_presplit_binding_rejects_mismatched_p2_hash() {
        let launcher_id = Bytes32::new([0xcc; 32]);
        let source_cat = Cat::new(
            chia_protocol::Coin::new(Bytes32::new([0x01; 32]), Bytes32::default(), 1000),
            None,
            chia_sdk_driver::CatInfo::new(Bytes32::new([0x02; 32]), None, Bytes32::default()),
        );
        let binding = PresplitOfferBinding::plan(
            launcher_id,
            RequestedPayments::new(),
            AssetInfo::new(),
            1000,
            None,
        )
        .expect("binding");
        let mismatched_cat = predict_presplit_cat(&source_cat, Bytes32::new([0x99; 32]), 1000);
        let err = verify_presplit_cat_offer_binding(&mismatched_cat, &binding).unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinPuzzleHashMismatch));
    }

    #[test]
    fn fixed_presplit_conditions_include_settlement_output() {
        let mut ctx = SpendContext::new();
        let requested_payments = RequestedPayments::new();
        let asset_info = AssetInfo::new();
        let fixed_spend = build_fixed_presplit_conditions_spend(
            &mut ctx,
            &requested_payments,
            &asset_info,
            1000,
            None,
        )
        .expect("fixed spend");
        let output =
            run_puzzle(&mut ctx, fixed_spend.puzzle, fixed_spend.solution).expect("run puzzle");
        let conditions = Conditions::<NodePtr>::from_clvm(&ctx, output).expect("conditions");
        assert!(
            conditions
                .iter()
                .any(|condition| matches!(condition, Condition::CreateCoin(create)
                    if create.puzzle_hash == SETTLEMENT_PAYMENT_HASH.into() && create.amount == 1000))
        );
    }
}
