//! Spend offered vault CAT coins back to vault change (offer cancel / reclaim).

use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{Cat, CatSpend, Offer, SpendContext, Vault};
use chia_sdk_types::Conditions;
use clvm_utils::TreeHash;

use crate::bech32m::decode_offer;
use crate::coinset::{OfferCoinsetBackend, OfferInputCatLookup};
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, hex_to_tree_hash};
use crate::offer::presplit::{
    build_presplit_offer_cancel_inner_spend, vault_change_puzzle_hash,
    verify_fixed_delegated_puzzle_hash_for_cat, PresplitOfferBinding,
};
use crate::offer::types::{OfferExecutionMode, PresplitCancelFields, StoredOfferCancelMetadata};
use crate::vault::materialize::{
    append_vault_singleton_spend_for_vault, build_vault_cat_inner_spend,
};
use crate::vault::spend::{VaultFastForwardSigner, VaultSpendContext};

#[derive(Debug, Clone, Copy)]
pub enum OfferCatReclaimMode {
    DirectVault,
    PresplitOffer {
        fixed_conditions_tree_hash: TreeHash,
    },
}

/// First offered CAT from a decoded offer (cancel spends one offered input).
///
/// # Errors
///
/// Returns an error when the offer has no offered CAT.
pub fn first_offered_cat(offer: &Offer) -> SignerResult<Cat> {
    for cats in offer.offered_coins().cats.values() {
        if let Some(cat) = cats.first() {
            return Ok(*cat);
        }
    }
    Err(SignerError::OfferCancelNoSpendableInput)
}

async fn fetch_input_cat_by_coin_id<C: OfferCoinsetBackend>(
    backend: &C,
    coin_id: Bytes32,
    offered_amount: u64,
) -> Option<Cat> {
    let cat = backend
        .fetch_offer_input_cat(OfferInputCatLookup::ByCoinId(coin_id))
        .await
        .ok()?;
    if cat.coin.amount == offered_amount {
        Some(cat)
    } else {
        None
    }
}

async fn resolve_offer_input_cat<C: OfferCoinsetBackend>(
    backend: &C,
    spend_bundle: &SpendBundle,
    offered: &Cat,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<Cat> {
    let fields = metadata.map(|value| &value.fields);
    if let Some(coin_id_hex) = fields
        .and_then(|value| value.input_coin_id.as_deref())
        .filter(|value| !value.is_empty())
    {
        if let Ok(coin_id) = hex_to_bytes32(coin_id_hex.trim()) {
            if let Some(cat) = fetch_input_cat_by_coin_id(backend, coin_id, offered.coin.amount).await
            {
                return Ok(cat);
            }
        }
    }
    let mut coin_ids = vec![offered.coin.coin_id()];
    for coin_spend in &spend_bundle.coin_spends {
        if coin_spend.coin.amount == offered.coin.amount {
            coin_ids.push(coin_spend.coin.coin_id());
        }
    }
    coin_ids.sort_unstable();
    coin_ids.dedup();
    for coin_id in coin_ids {
        if let Some(cat) = fetch_input_cat_by_coin_id(backend, coin_id, offered.coin.amount).await {
            return Ok(cat);
        }
    }
    backend
        .fetch_offer_input_cat(OfferInputCatLookup::ByCatFingerprint {
            asset_id: offered.info.asset_id,
            inner_puzzle_hash: offered.info.p2_puzzle_hash,
            amount: offered.coin.amount,
        })
        .await
        .map_err(|err| match err {
            SignerError::PresplitCoinNotFound => SignerError::OfferCancelInputCoinAlreadySpent,
            other => other,
        })
}

fn presplit_fixed_conditions_tree_hash(
    launcher_id: Bytes32,
    cat: &Cat,
    spend_bundle: &SpendBundle,
    fields: Option<&PresplitCancelFields>,
) -> SignerResult<TreeHash> {
    if let Some(hash_hex) = fields
        .and_then(|value| value.fixed_delegated_puzzle_hash.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let hash = hex_to_tree_hash(hash_hex)?;
        verify_fixed_delegated_puzzle_hash_for_cat(launcher_id, cat, hash)?;
        return Ok(hash);
    }
    PresplitOfferBinding::from_presplit_input_spend(launcher_id, cat, spend_bundle)
        .map(|binding| binding.fixed_conditions_tree_hash)
}

fn resolve_reclaim_mode(
    vault_ctx: &mut VaultSpendContext,
    cat: &Cat,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<OfferCatReclaimMode> {
    let fields = metadata.map(|value| &value.fields);
    let execution_mode = metadata.and_then(|value| value.execution_mode);
    match execution_mode {
        Some(OfferExecutionMode::Direct) => Ok(OfferCatReclaimMode::DirectVault),
        Some(OfferExecutionMode::PresplitNew | OfferExecutionMode::PresplitExisting) => {
            Ok(OfferCatReclaimMode::PresplitOffer {
                fixed_conditions_tree_hash: presplit_fixed_conditions_tree_hash(
                    vault_ctx.launcher_id,
                    cat,
                    spend_bundle,
                    fields,
                )?,
            })
        }
        None => {
            if vault_ctx
                .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
                .is_some()
            {
                Ok(OfferCatReclaimMode::DirectVault)
            } else {
                Ok(OfferCatReclaimMode::PresplitOffer {
                    fixed_conditions_tree_hash: presplit_fixed_conditions_tree_hash(
                        vault_ctx.launcher_id,
                        cat,
                        spend_bundle,
                        fields,
                    )?,
                })
            }
        }
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
    mode: OfferCatReclaimMode,
    vault: &Vault,
    sign_digest: F,
) -> SignerResult<SpendBundle>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<chia_secp::R1Signature>>,
{
    let mut ctx = SpendContext::new();
    let memos = ctx.hint(change_puzzle_hash).map_err(SignerError::from)?;
    let conditions = Conditions::new().create_coin(change_puzzle_hash, cat.coin.amount, memos);
    let delegated = ctx.delegated_spend(conditions).map_err(SignerError::from)?;
    let inner_spend = match mode {
        OfferCatReclaimMode::DirectVault => {
            let nonce = vault_ctx
                .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
                .ok_or(SignerError::Driver(
                    "failed to infer vault nonce for reclaim cat".to_string(),
                ))?;
            build_vault_cat_inner_spend(
                &mut ctx,
                delegated,
                vault_ctx,
                nonce,
                cat.info.p2_puzzle_hash.into(),
            )?
        }
        OfferCatReclaimMode::PresplitOffer {
            fixed_conditions_tree_hash,
        } => build_presplit_offer_cancel_inner_spend(
            &mut ctx,
            delegated,
            vault_ctx,
            fixed_conditions_tree_hash,
        )?,
    };
    Cat::spend_all(&mut ctx, &[CatSpend::new(cat, inner_spend)]).map_err(SignerError::from)?;
    append_vault_singleton_spend_for_vault(&mut ctx, vault_ctx, vault, sign_digest).await?;
    Ok(SpendBundle::new(ctx.take(), chia_bls::Signature::default()))
}

/// Build an on-chain offer cancel spend bundle from offer file text.
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
    let offered_cat = first_offered_cat(&offer)?;
    let cat = resolve_offer_input_cat(backend, &spend_bundle, &offered_cat, metadata).await?;
    let change_puzzle_hash = vault_change_puzzle_hash(vault_ctx.launcher_id)?;
    let vault = backend
        .fetch_latest_vault(vault_ctx.launcher_id, vault_ctx.inner_puzzle_hash)
        .await?;
    let signer = VaultFastForwardSigner::from_context(vault_ctx);
    let mode = resolve_reclaim_mode(vault_ctx, &cat, &spend_bundle, metadata)?;
    build_vault_cat_reclaim_spend_bundle(
        vault_ctx,
        cat,
        change_puzzle_hash,
        mode,
        &vault,
        move |message| {
            let signer = signer.clone();
            async move { signer.sign(message).await }
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clvmr::Allocator;

    #[test]
    fn first_offered_cat_requires_offered_cat() {
        let mut allocator = Allocator::new();
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        let offer = Offer::from_spend_bundle(&mut allocator, &bundle).expect("offer");
        let err = first_offered_cat(&offer).unwrap_err();
        assert!(matches!(err, SignerError::OfferCancelNoSpendableInput));
    }
}
