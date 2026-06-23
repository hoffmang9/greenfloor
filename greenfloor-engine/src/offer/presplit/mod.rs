mod cancel_binding;

use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{
    Cat, CatSpend, InnerPuzzleSpend, MipsSpend, Offer, Spend, SpendContext, Vault,
};
use chia_sdk_types::puzzles::{SingletonMember, SingletonMemberSolution};
use chia_sdk_types::Conditions;
use clvm_utils::TreeHash;
use clvmr::{Allocator, NodePtr};

use crate::bech32m::encode_offer;
use crate::coinset::{spend_bundle_hex, OfferCoinsetBackend};
use crate::error::{SignerError, SignerResult};
use crate::offer::plan::{
    build_offer_payment_bundle, build_offer_request_conditions, OfferPaymentBundle,
};
use crate::offer::types::OfferTerms;
use crate::vault::materialize::build_vault_cat_inner_spend;
use crate::vault::members::{nonce_member_puzzle_hash, p2_conditions_or_singleton_puzzle_hash};
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

pub(crate) use cancel_binding::verify_fixed_delegated_puzzle_hash_for_cat;

#[must_use]
pub fn offer_nonce_from_cats(cats: &[Cat]) -> Bytes32 {
    Offer::nonce(cats.iter().map(|cat| cat.coin.coin_id()).collect())
}

#[must_use]
pub fn offer_nonce_from_coin_ids(coin_ids: &[Bytes32]) -> Bytes32 {
    Offer::nonce(coin_ids.to_vec())
}

/// Build fixed presplit conditions spend.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) fn build_fixed_presplit_conditions_spend(
    ctx: &mut SpendContext,
    payments: &OfferPaymentBundle,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<Spend> {
    let mut conditions = build_offer_request_conditions(ctx, payments, expires_at)?;
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

/// Build presplit conditions inner spend.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn build_presplit_conditions_inner_spend(
    ctx: &mut SpendContext,
    fixed_spend: Spend,
    launcher_id: Bytes32,
) -> SignerResult<Spend> {
    let hashes =
        p2_conditions_or_singleton_puzzle_hash(ctx.tree_hash(fixed_spend.puzzle), launcher_id)?;
    let fixed_conditions_hash = hashes.fixed_conditions_hash;
    let p2_singleton_hash = hashes.p2_singleton_hash;
    let full_puzzle_hash = hashes.puzzle_hash;

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

/// Inner spend for presplit offer CAT cancel via the singleton branch.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) fn build_presplit_offer_cancel_inner_spend(
    ctx: &mut SpendContext,
    cancel_delegated: Spend,
    vault_ctx: &VaultSpendContext,
    fixed_delegated_puzzle_hash: TreeHash,
) -> SignerResult<Spend> {
    let hashes =
        p2_conditions_or_singleton_puzzle_hash(fixed_delegated_puzzle_hash, vault_ctx.launcher_id)?;
    let fixed_conditions_hash = hashes.fixed_conditions_hash;
    let p2_singleton_hash = hashes.p2_singleton_hash;
    let full_puzzle_hash = hashes.puzzle_hash;

    let mut mips_spend = MipsSpend::new(cancel_delegated);
    let member = SingletonMember::new(vault_ctx.launcher_id);
    let member_puzzle = ctx.curry(member).map_err(SignerError::from)?;
    let member_solution = ctx
        .alloc(&SingletonMemberSolution::new(
            vault_ctx.inner_puzzle_hash.into(),
            1,
        ))
        .map_err(SignerError::from)?;
    mips_spend.members.insert(
        p2_singleton_hash,
        InnerPuzzleSpend::new(0, Vec::new(), Spend::new(member_puzzle, member_solution)),
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

/// Vault change puzzle hash.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn vault_change_puzzle_hash(launcher_id: Bytes32) -> SignerResult<Bytes32> {
    Ok(nonce_member_puzzle_hash(launcher_id, 0)?.into())
}

#[derive(Debug, Clone)]
pub struct PresplitOfferBinding {
    pub offer_amount: u64,
    pub expires_at: Option<u64>,
    pub fixed_conditions_tree_hash: TreeHash,
    pub p2_puzzle_hash: Bytes32,
}

/// Fixed presplit conditions built in one spend context.
struct PresplitFixedSpend {
    fixed_spend: Spend,
    fixed_conditions_tree_hash: TreeHash,
}

/// Build requested payments and presplit fixed conditions in one [`SpendContext`].
///
/// CAT quote memos are allocator-backed node pointers and must stay in the same
/// context through assertion tree hashing.
fn build_presplit_fixed_spend(
    ctx: &mut SpendContext,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
    offer_amount: u64,
    expires_at: Option<u64>,
) -> SignerResult<PresplitFixedSpend> {
    let payments = build_offer_payment_bundle(ctx, terms, receive_puzzle_hash, offer_nonce)?;
    let fixed_spend =
        build_fixed_presplit_conditions_spend(ctx, &payments, offer_amount, expires_at)?;
    Ok(PresplitFixedSpend {
        fixed_conditions_tree_hash: ctx.tree_hash(fixed_spend.puzzle),
        fixed_spend,
    })
}

/// Encode the final offer after presplit input spends are materialized.
///
/// Presplit offer assembly needs a fresh [`SpendContext`]: the spend context is
/// consumed by [`SpendContext::take`] when building the input bundle, and
/// [`Offer::to_spend_bundle`] needs its own allocator-backed payment nodes.
fn encode_presplit_offer_from_input(
    input_spend_bundle: SpendBundle,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String)> {
    let mut offer_ctx = SpendContext::new();
    let offer_payments =
        build_offer_payment_bundle(&mut offer_ctx, terms, receive_puzzle_hash, offer_nonce)?;
    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle,
        offer_payments.requested_payments,
        offer_payments.requested_asset_info,
    )
    .map_err(SignerError::from)?;
    let offer_spend_bundle = offer
        .to_spend_bundle(&mut offer_ctx)
        .map_err(SignerError::from)?;
    let offer_text = encode_offer(&offer_spend_bundle)?;
    let spend_bundle_hex = spend_bundle_hex(&offer_spend_bundle)?;
    Ok((offer_text, spend_bundle_hex))
}

impl PresplitOfferBinding {
    /// Plan presplit fixed conditions and P2 puzzle hash using one spend context.
    ///
    /// This is the first of up to three payment rebuilds in a presplit-new offer:
    /// plan here (hash only), rebuild to verify binding and spend the presplit CAT,
    /// then rebuild again in a fresh context for final offer encoding. Each rebuild
    /// is required by allocator lifetimes or hash verification, not accidental duplication.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub(crate) fn plan(
        launcher_id: Bytes32,
        terms: &OfferTerms,
        receive_puzzle_hash: Bytes32,
        offer_nonce: Bytes32,
    ) -> SignerResult<Self> {
        let mut ctx = SpendContext::new();
        let built = build_presplit_fixed_spend(
            &mut ctx,
            terms,
            receive_puzzle_hash,
            offer_nonce,
            terms.offer_amount,
            terms.expires_at,
        )?;
        let p2_hashes =
            p2_conditions_or_singleton_puzzle_hash(built.fixed_conditions_tree_hash, launcher_id)?;
        Ok(Self {
            offer_amount: terms.offer_amount,
            expires_at: terms.expires_at,
            fixed_conditions_tree_hash: built.fixed_conditions_tree_hash,
            p2_puzzle_hash: p2_hashes.puzzle_hash.into(),
        })
    }

    /// Reconstruct presplit binding from a decoded offer (cancel / reclaim path).
    ///
    /// # Errors
    ///
    /// Returns an error if offer terms cannot be inferred or binding planning fails.
    pub(crate) fn from_presplit_input_spend(
        launcher_id: Bytes32,
        chain_cat: &Cat,
        spend_bundle: &SpendBundle,
    ) -> SignerResult<Self> {
        let fixed_conditions_tree_hash =
            cancel_binding::presplit_fixed_conditions_tree_hash_from_input(
                launcher_id,
                chain_cat,
                spend_bundle,
            )?;
        Ok(Self {
            offer_amount: chain_cat.coin.amount,
            expires_at: None,
            fixed_conditions_tree_hash,
            p2_puzzle_hash: chain_cat.info.p2_puzzle_hash,
        })
    }
}

/// Verify presplit cat offer binding.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn verify_presplit_cat_offer_binding(
    presplit_cat: &Cat,
    binding: &PresplitOfferBinding,
) -> SignerResult<()> {
    if presplit_cat.info.p2_puzzle_hash != binding.p2_puzzle_hash {
        return Err(SignerError::PresplitCoinPuzzleHashMismatch);
    }
    Ok(())
}

/// Build presplit split spend bundle.
///
/// # Errors
///
/// Returns an error if the operation fails.
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

/// Validate presplit source cats.
///
/// # Errors
///
/// Returns an error if the operation fails.
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

/// Build offer from presplit cat.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn build_offer_from_presplit_cat(
    presplit_cat: Cat,
    launcher_id: Bytes32,
    binding: PresplitOfferBinding,
    terms: &OfferTerms,
    receive_puzzle_hash: Bytes32,
    offer_nonce: Bytes32,
) -> SignerResult<(String, String, String)> {
    // Rebuild fixed conditions in one spend context, verify against the binding
    // planned before the vault split, then materialize the presplit CAT input spend.
    let mut ctx = SpendContext::new();
    let built = build_presplit_fixed_spend(
        &mut ctx,
        terms,
        receive_puzzle_hash,
        offer_nonce,
        binding.offer_amount,
        binding.expires_at,
    )?;
    if built.fixed_conditions_tree_hash != binding.fixed_conditions_tree_hash {
        return Err(SignerError::Driver(
            "presplit fixed conditions hash mismatch".to_string(),
        ));
    }
    let inner_spend =
        build_presplit_conditions_inner_spend(&mut ctx, built.fixed_spend, launcher_id)?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(presplit_cat, inner_spend)])
        .map_err(SignerError::from)?;
    let input_spend_bundle = SpendBundle::new(ctx.take(), chia_bls::Signature::default());

    let (offer_text, spend_bundle_hex) = encode_presplit_offer_from_input(
        input_spend_bundle,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )?;
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

    use crate::offer::types::OfferTerms;

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
        let terms = OfferTerms {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
            offer_asset_id: hex::encode(Bytes32::new([0x02; 32])),
            offer_amount: 1000,
            request_asset_id: "xch".to_string(),
            request_amount: 1,
            expires_at: None,
        };
        let binding =
            PresplitOfferBinding::plan(launcher_id, &terms, Bytes32::default(), Bytes32::default())
                .expect("binding");
        let mismatched_cat = predict_presplit_cat(&source_cat, Bytes32::new([0x99; 32]), 1000);
        let err = verify_presplit_cat_offer_binding(&mismatched_cat, &binding).unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinPuzzleHashMismatch));
    }

    #[test]
    fn fixed_presplit_conditions_include_settlement_output() {
        let mut ctx = SpendContext::new();
        let payments = OfferPaymentBundle {
            requested_payments: RequestedPayments::new(),
            requested_asset_info: AssetInfo::new(),
        };
        let fixed_spend = build_fixed_presplit_conditions_spend(&mut ctx, &payments, 1000, None)
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
