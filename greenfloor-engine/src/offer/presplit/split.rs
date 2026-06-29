use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Cat, CatSpend, SpendContext, Vault};
use chia_sdk_types::Conditions;

use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use crate::vault::materialize::{
    append_vault_singleton_spend_for_vault, build_vault_cat_inner_spend,
};
use crate::vault::members::nonce_member_puzzle_hash;
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

/// Validate presplit source cats.
///
/// # Errors
///
/// Returns an error when more or fewer than one source CAT is provided.
pub fn validate_presplit_source_cats(source_cat_count: usize) -> SignerResult<()> {
    if source_cat_count != 1 {
        return Err(SignerError::PresplitRequiresSingleSourceCat);
    }
    Ok(())
}

/// Vault nonce-0 member puzzle hash used as presplit split change destination.
///
/// # Errors
///
/// Returns an error if the vault member puzzle hash cannot be derived.
pub fn vault_change_puzzle_hash(launcher_id: Bytes32) -> SignerResult<Bytes32> {
    Ok(nonce_member_puzzle_hash(launcher_id, 0)?.into())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PresplitSplitParams {
    pub change_puzzle_hash: Bytes32,
    pub p2_puzzle_hash: Bytes32,
    pub offer_amount: u64,
    pub change_amount: u64,
}

/// Build presplit split spend bundle.
///
/// # Errors
///
/// Returns an error if vault lookup, signing, or spend construction fails.
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
    let presplit_cat = source_cat.child(params.p2_puzzle_hash, params.offer_amount);

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
    append_vault_singleton_spend_for_vault(&mut ctx, vault_ctx, &vault, sign_digest).await?;
    Ok((
        SpendBundle::new(ctx.take(), chia_bls::Signature::default()),
        presplit_cat,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presplit_requires_single_source_cat() {
        let err = validate_presplit_source_cats(2).unwrap_err();
        assert!(matches!(err, SignerError::PresplitRequiresSingleSourceCat));
    }
}
