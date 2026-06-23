//! Spend offer-locked vault maker coins back to vault change (offer cancel / reclaim).
//!
//! Production offers use presplit maker inputs (`split_input_coins`) for XCH and CAT so maker
//! coins sit at `P2_CONDITIONS_OR_SINGLETON` and the published offer bundle is self-contained.
//! Cancel walks [`Offer::cancellable_coin_spends`], not settlement/notary side coins.

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::{Cat, CatSpend, Offer, SpendContext, Vault};
use clvm_utils::TreeHash;

use crate::bech32m::decode_offer;
use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use crate::offer::cancel_input::{classify_cancellable_maker_input, CancellableMakerInput};
use crate::offer::presplit::{build_presplit_offer_cancel_inner_spend, vault_change_puzzle_hash};
use crate::offer::types::StoredOfferCancelMetadata;
use crate::vault::materialize::{
    append_vault_p2_reclaim_spend, build_vault_change_delegated_spend,
    build_vault_change_inner_spend, finalize_vault_reclaim_spend_bundle,
};
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

pub use crate::offer::cancel_input::OfferReclaimMode;

fn build_presplit_reclaim_inner_spend(
    ctx: &mut SpendContext,
    change_puzzle_hash: Bytes32,
    amount: u64,
    vault_ctx: &VaultSpendContext,
    fixed_conditions_tree_hash: TreeHash,
) -> SignerResult<chia_sdk_driver::Spend> {
    let delegated = build_vault_change_delegated_spend(ctx, change_puzzle_hash, amount)?;
    build_presplit_offer_cancel_inner_spend(ctx, delegated, vault_ctx, fixed_conditions_tree_hash)
}

fn append_presplit_reclaim_to_context(
    ctx: &mut SpendContext,
    coin: Coin,
    cat: Option<Cat>,
    change_puzzle_hash: Bytes32,
    vault_ctx: &VaultSpendContext,
    fixed_conditions_tree_hash: TreeHash,
) -> SignerResult<()> {
    let amount = cat.map_or(coin.amount, |value| value.coin.amount);
    let inner_spend = build_presplit_reclaim_inner_spend(
        ctx,
        change_puzzle_hash,
        amount,
        vault_ctx,
        fixed_conditions_tree_hash,
    )?;
    if let Some(cat) = cat {
        Cat::spend_all(ctx, &[CatSpend::new(cat, inner_spend)]).map_err(SignerError::from)?;
    } else {
        ctx.spend(coin, inner_spend).map_err(SignerError::from)?;
    }
    Ok(())
}

fn append_cancellable_input_reclaim(
    ctx: &mut SpendContext,
    input: &CancellableMakerInput,
    change_puzzle_hash: Bytes32,
    vault_ctx: &mut VaultSpendContext,
) -> SignerResult<()> {
    match *input {
        CancellableMakerInput::DirectVaultP2 { coin, nonce } => append_vault_p2_reclaim_spend(
            ctx,
            coin,
            change_puzzle_hash,
            vault_ctx,
            coin.puzzle_hash.into(),
            nonce,
        ),
        CancellableMakerInput::VaultCat { cat, mode } => match mode {
            OfferReclaimMode::DirectVault => {
                let nonce = vault_ctx
                    .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
                    .ok_or(SignerError::Driver(
                        "failed to infer vault nonce for reclaim cat".to_string(),
                    ))?;
                let inner_spend = build_vault_change_inner_spend(
                    ctx,
                    change_puzzle_hash,
                    cat.coin.amount,
                    vault_ctx,
                    nonce,
                    cat.info.p2_puzzle_hash.into(),
                )?;
                Cat::spend_all(ctx, &[CatSpend::new(cat, inner_spend)])
                    .map_err(SignerError::from)?;
                Ok(())
            }
            OfferReclaimMode::PresplitOffer {
                fixed_conditions_tree_hash,
            } => append_presplit_reclaim_to_context(
                ctx,
                cat.coin,
                Some(cat),
                change_puzzle_hash,
                vault_ctx,
                fixed_conditions_tree_hash,
            ),
        },
        CancellableMakerInput::PresplitVaultXch {
            coin,
            fixed_conditions_tree_hash,
        } => append_presplit_reclaim_to_context(
            ctx,
            coin,
            None,
            change_puzzle_hash,
            vault_ctx,
            fixed_conditions_tree_hash,
        ),
    }
}

/// Build a spend bundle that returns an offered CAT coin to vault change.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_vault_cat_reclaim_spend_bundle<F, Fut>(
    vault_ctx: &mut VaultSpendContext,
    cat: Cat,
    change_puzzle_hash: Bytes32,
    mode: OfferReclaimMode,
    vault: &Vault,
    sign_digest: F,
) -> SignerResult<SpendBundle>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<chia_secp::R1Signature>>,
{
    let mut ctx = SpendContext::new();
    append_cancellable_input_reclaim(
        &mut ctx,
        &CancellableMakerInput::VaultCat { cat, mode },
        change_puzzle_hash,
        vault_ctx,
    )?;
    finalize_vault_reclaim_spend_bundle(ctx, vault_ctx, vault, sign_digest).await
}

/// Build an on-chain offer cancel spend bundle from offer file text.
///
/// Spends every [`Offer::cancellable_coin_spends`] input back to vault change.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_offer_cancel_spend_bundle<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    backend: &C,
    offer_text: &str,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<SpendBundle> {
    let spend_bundle = decode_offer(offer_text)?;
    let mut allocator = clvmr::Allocator::new();
    let offer = Offer::from_spend_bundle(&mut allocator, &spend_bundle)?;
    let cancellable = offer.cancellable_coin_spends().map_err(SignerError::from)?;
    if cancellable.is_empty() {
        return Err(SignerError::OfferCancelNoSpendableInput);
    }

    let change_puzzle_hash = vault_change_puzzle_hash(vault_ctx.launcher_id)?;
    let vault = backend
        .fetch_latest_vault(vault_ctx.launcher_id, vault_ctx.inner_puzzle_hash)
        .await?;
    let signer = VaultFastForwardSigner::from_context(vault_ctx);
    let mut ctx = SpendContext::new();

    for coin_spend in &cancellable {
        let input = classify_cancellable_maker_input(
            vault_ctx,
            backend,
            &spend_bundle,
            metadata,
            coin_spend.coin,
        )
        .await?;
        append_cancellable_input_reclaim(&mut ctx, &input, change_puzzle_hash, vault_ctx)?;
    }

    finalize_vault_reclaim_spend_bundle(ctx, vault_ctx, &vault, move |message| {
        let signer = signer.clone();
        async move { signer.sign(message).await }
    })
    .await
}
