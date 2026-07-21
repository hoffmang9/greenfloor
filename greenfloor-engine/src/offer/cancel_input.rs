//! Classify cancellable offer maker coins and resolve on-chain CAT inputs for cancel.

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::Cat;
use clvm_utils::TreeHash;

use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, hex_to_tree_hash};
use crate::offer::presplit::{
    offer_maker_cat_from_coin_input, presplit_binding_from_coin_input,
    verify_fixed_delegated_puzzle_hash_for_binding, PresplitBindingLookup,
};
use crate::offer::types::{OfferCancelFields, OfferExecutionMode, StoredOfferCancelMetadata};
use crate::vault::spend::VaultSpendContext;

/// How a vault-owned maker coin should be reclaimed to vault change.
#[derive(Debug, Clone, Copy)]
pub enum OfferReclaimMode {
    DirectVault,
    PresplitOffer {
        fixed_conditions_tree_hash: TreeHash,
    },
}

/// Classified cancellable maker input for offer cancel / reclaim.
#[derive(Debug, Clone, Copy)]
pub enum CancellableMakerInput {
    DirectVaultP2 {
        coin: Coin,
        nonce: u32,
    },
    VaultCatDirect {
        cat: Cat,
    },
    PresplitMaker {
        coin: Coin,
        cat: Option<Cat>,
        fixed_conditions_tree_hash: TreeHash,
    },
}

async fn fetch_input_cat_by_coin_id<C: OfferCoinsetBackend>(
    backend: &C,
    coin_id: Bytes32,
    offered_amount: u64,
) -> SignerResult<Option<Cat>> {
    let cat = backend.fetch_offer_input_cat(coin_id).await?;
    if cat.coin.amount == offered_amount {
        Ok(Some(cat))
    } else {
        Ok(None)
    }
}

fn stored_input_coin_id(
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<Option<Bytes32>> {
    let Some(coin_id_hex) = metadata
        .and_then(|value| value.fields.input_coin_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(hex_to_bytes32(coin_id_hex)?))
}

/// Fixed-delegated cancel fields when stored metadata is usable for presplit cancel.
pub(crate) fn stored_presplit_fields(
    metadata: Option<&StoredOfferCancelMetadata>,
) -> Option<&OfferCancelFields> {
    let metadata = metadata?;
    let hash = metadata
        .fields
        .fixed_delegated_puzzle_hash
        .as_deref()?
        .trim();
    if hash.is_empty() {
        return None;
    }
    match metadata.execution_mode {
        Some(OfferExecutionMode::Direct) => None,
        Some(OfferExecutionMode::PresplitNew | OfferExecutionMode::PresplitExisting) => {
            Some(&metadata.fields)
        }
        None => Some(&metadata.fields),
    }
}

/// Whether stored cancel metadata can drive Coinset-primary cancel without an offer file.
#[must_use]
pub fn metadata_sufficient_for_coinset_cancel(
    metadata: Option<&StoredOfferCancelMetadata>,
) -> bool {
    let Some(metadata) = metadata else {
        return false;
    };
    let has_coin = metadata
        .fields
        .input_coin_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    has_coin || stored_presplit_fields(Some(metadata)).is_some()
}

fn coin_id_candidates_for_cat_resolution(
    coin: Coin,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<Vec<Bytes32>> {
    let mut coin_ids = Vec::new();
    if let Some(stored) = stored_input_coin_id(metadata)? {
        coin_ids.push(stored);
    }
    coin_ids.push(coin.coin_id());
    for coin_spend in &spend_bundle.coin_spends {
        if coin_spend.coin.amount == coin.amount {
            coin_ids.push(coin_spend.coin.coin_id());
        }
    }
    coin_ids.sort_unstable();
    coin_ids.dedup();
    Ok(coin_ids)
}

/// Resolve an on-chain CAT matching a cancellable maker coin, if any.
pub(crate) async fn resolve_cancellable_cat<C: OfferCoinsetBackend>(
    backend: &C,
    spend_bundle: &SpendBundle,
    coin: Coin,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<Option<Cat>> {
    for coin_id in coin_id_candidates_for_cat_resolution(coin, spend_bundle, metadata)? {
        match fetch_input_cat_by_coin_id(backend, coin_id, coin.amount).await {
            Ok(Some(cat)) if cat.coin.coin_id() == coin.coin_id() => return Ok(Some(cat)),
            Ok(_) | Err(SignerError::PresplitCoinNotFound) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(None)
}

fn presplit_hash_from_stored_fields(
    launcher_id: Bytes32,
    coin: Coin,
    cat: Option<Cat>,
    fields: &OfferCancelFields,
) -> SignerResult<TreeHash> {
    let hash = hex_to_tree_hash(
        fields
            .fixed_delegated_puzzle_hash
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(SignerError::OfferCancelNoSpendableInput)?,
    )?;
    let binding_p2 = cat.map_or(coin.puzzle_hash, |value| value.info.p2_puzzle_hash);
    verify_fixed_delegated_puzzle_hash_for_binding(launcher_id, binding_p2, hash)?;
    Ok(hash)
}

/// Shared direct/presplit decision for a known on-chain coin (+ optional CAT).
fn classify_coin_and_cat(
    vault_ctx: &mut VaultSpendContext,
    coin: Coin,
    cat: Option<Cat>,
    metadata: Option<&StoredOfferCancelMetadata>,
    offer_bundle: Option<&SpendBundle>,
) -> SignerResult<CancellableMakerInput> {
    if let Some(nonce) = vault_ctx.infer_nonce_for_p2_hash(coin.puzzle_hash) {
        return Ok(CancellableMakerInput::DirectVaultP2 { coin, nonce });
    }

    if let Some(cat) = cat {
        if metadata
            .and_then(|value| value.execution_mode)
            .is_some_and(|mode| mode == OfferExecutionMode::Direct)
        {
            return Ok(CancellableMakerInput::VaultCatDirect { cat });
        }
        let presplit_by_mode = metadata.is_some_and(|value| {
            value.execution_mode.is_some_and(|mode| {
                matches!(
                    mode,
                    OfferExecutionMode::PresplitNew | OfferExecutionMode::PresplitExisting
                )
            })
        });
        let direct_by_nonce = vault_ctx
            .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
            .is_some();
        if !presplit_by_mode && direct_by_nonce {
            return Ok(CancellableMakerInput::VaultCatDirect { cat });
        }
        return resolve_presplit_maker(
            vault_ctx.launcher_id,
            coin,
            Some(cat),
            metadata,
            offer_bundle,
        )?
        .ok_or(SignerError::OfferCancelNoSpendableInput);
    }

    resolve_presplit_maker(vault_ctx.launcher_id, coin, None, metadata, offer_bundle)?
        .ok_or_else(|| offer_cancel_input_not_vault_owned(coin, vault_ctx.launcher_id))
}

fn resolve_presplit_maker(
    launcher_id: Bytes32,
    coin: Coin,
    coinset_cat: Option<Cat>,
    metadata: Option<&StoredOfferCancelMetadata>,
    offer_bundle: Option<&SpendBundle>,
) -> SignerResult<Option<CancellableMakerInput>> {
    if let Some(fields) = stored_presplit_fields(metadata) {
        return match presplit_hash_from_stored_fields(launcher_id, coin, coinset_cat, fields) {
            Ok(fixed_conditions_tree_hash) => {
                let cat = match coinset_cat {
                    Some(cat) => Some(cat),
                    None => match offer_bundle {
                        Some(bundle) => offer_maker_cat_from_coin_input(coin, bundle)?,
                        None => None,
                    },
                };
                Ok(Some(CancellableMakerInput::PresplitMaker {
                    coin,
                    cat,
                    fixed_conditions_tree_hash,
                }))
            }
            Err(SignerError::PresplitCoinPuzzleHashMismatch) => {
                Err(offer_cancel_input_not_vault_owned(coin, launcher_id))
            }
            Err(err) => Err(err),
        };
    }
    let Some(bundle) = offer_bundle else {
        return Ok(None);
    };
    match presplit_binding_from_coin_input(launcher_id, coin, bundle) {
        Ok(PresplitBindingLookup::Found(binding)) => {
            Ok(Some(CancellableMakerInput::PresplitMaker {
                coin,
                cat: coinset_cat.or(binding.parsed_cat),
                fixed_conditions_tree_hash: binding.fixed_conditions_tree_hash,
            }))
        }
        Ok(PresplitBindingLookup::NotPresplitMaker) => Ok(None),
        Err(SignerError::PresplitCoinPuzzleHashMismatch) => {
            Err(offer_cancel_input_not_vault_owned(coin, launcher_id))
        }
        Err(err) => Err(err),
    }
}

fn offer_cancel_input_not_vault_owned(coin: Coin, launcher_id: Bytes32) -> SignerError {
    SignerError::OfferCancelInputNotVaultOwned {
        coin_id: hex::encode(coin.coin_id()),
        puzzle_hash: hex::encode(coin.puzzle_hash),
        launcher_id: hex::encode(launcher_id),
    }
}

async fn ensure_offer_input_unspent<C: OfferCoinsetBackend>(
    backend: &C,
    coin_id: Bytes32,
) -> SignerResult<()> {
    if backend.offer_input_coin_is_spent(coin_id).await? {
        return Err(SignerError::OfferCancelInputCoinAlreadySpent);
    }
    Ok(())
}

fn direct_vault_cat_missing_on_coinset(
    vault_ctx: &mut VaultSpendContext,
    coin: Coin,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<bool> {
    let Some(cat) = offer_maker_cat_from_coin_input(coin, spend_bundle)? else {
        return Ok(false);
    };
    if metadata
        .and_then(|value| value.execution_mode)
        .is_some_and(|mode| mode == OfferExecutionMode::Direct)
    {
        return Ok(true);
    }
    Ok(vault_ctx
        .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
        .is_some())
}

/// Classify a cancellable maker coin from an offer spend bundle.
///
/// Presplit hash resolution prefers persisted cancel metadata, then offer-file binding parse.
///
/// # Errors
///
/// Returns [`SignerError::OfferCancelInputNotVaultOwned`] when the coin is not vault-owned.
/// Returns [`SignerError::OfferCancelInputCoinAlreadySpent`] when coinset shows the maker
/// input is missing or already spent.
pub(crate) async fn classify_cancellable_maker_input<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    backend: &C,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
    coin: Coin,
) -> SignerResult<CancellableMakerInput> {
    if let Some(nonce) = vault_ctx.infer_nonce_for_p2_hash(coin.puzzle_hash) {
        ensure_offer_input_unspent(backend, coin.coin_id()).await?;
        return Ok(CancellableMakerInput::DirectVaultP2 { coin, nonce });
    }

    if let Some(cat) = resolve_cancellable_cat(backend, spend_bundle, coin, metadata).await? {
        ensure_offer_input_unspent(backend, cat.coin.coin_id()).await?;
        return classify_coin_and_cat(vault_ctx, coin, Some(cat), metadata, Some(spend_bundle));
    }

    if let Some(input) = resolve_presplit_maker(
        vault_ctx.launcher_id,
        coin,
        None,
        metadata,
        Some(spend_bundle),
    )? {
        ensure_offer_input_unspent(backend, coin.coin_id()).await?;
        return Ok(input);
    }

    if direct_vault_cat_missing_on_coinset(vault_ctx, coin, spend_bundle, metadata)? {
        return Err(SignerError::OfferCancelInputCoinAlreadySpent);
    }

    Err(offer_cancel_input_not_vault_owned(
        coin,
        vault_ctx.launcher_id,
    ))
}

/// Classify a maker input from Coinset coin state + stored cancel metadata (no offer file).
///
/// # Errors
///
/// Returns an error when the stored coin id is missing/spent or cannot be classified as
/// a vault-owned reclaim input.
pub(crate) async fn classify_maker_input_from_stored_metadata<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    backend: &C,
    metadata: &StoredOfferCancelMetadata,
) -> SignerResult<CancellableMakerInput> {
    let coin_id = stored_input_coin_id(Some(metadata))?.ok_or_else(|| {
        SignerError::Other(
            "offer cancel requires stored presplit_input_coin_id when offer file is unavailable"
                .to_string(),
        )
    })?;
    ensure_offer_input_unspent(backend, coin_id).await?;

    match backend.fetch_offer_input_cat(coin_id).await {
        Ok(cat) => {
            return classify_coin_and_cat(vault_ctx, cat.coin, Some(cat), Some(metadata), None);
        }
        Err(SignerError::PresplitCoinNotFound) => {}
        Err(err) => return Err(err),
    }

    let coin = backend.fetch_unspent_offer_input_coin(coin_id).await?;
    classify_coin_and_cat(vault_ctx, coin, None, Some(metadata), None)
}
