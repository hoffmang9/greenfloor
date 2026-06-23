//! Classify cancellable offer maker coins and resolve on-chain CAT inputs for cancel.

use chia_protocol::{Bytes32, Coin, SpendBundle};
use chia_sdk_driver::Cat;
use clvm_utils::TreeHash;

use crate::coinset::OfferCoinsetBackend;
use crate::error::{SignerError, SignerResult};
use crate::hex::{hex_to_bytes32, hex_to_tree_hash};
use crate::offer::presplit::{
    presplit_binding_from_coin_input, verify_fixed_delegated_puzzle_hash_for_binding,
};
use crate::offer::types::{OfferExecutionMode, PresplitCancelFields, StoredOfferCancelMetadata};
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
    VaultCat {
        cat: Cat,
        mode: OfferReclaimMode,
    },
    PresplitVaultXch {
        coin: Coin,
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

fn coin_id_candidates_for_cat_resolution(
    coin: Coin,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<Vec<Bytes32>> {
    let mut coin_ids = Vec::new();
    if let Some(stored) = stored_input_coin_id(metadata)? {
        if stored == coin.coin_id() {
            coin_ids.push(stored);
        }
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

fn presplit_fixed_conditions_tree_hash(
    launcher_id: Bytes32,
    coin: Coin,
    cat: Option<Cat>,
    spend_bundle: &SpendBundle,
    fields: Option<&PresplitCancelFields>,
) -> SignerResult<TreeHash> {
    if let Some(hash_hex) = fields
        .and_then(|value| value.fixed_delegated_puzzle_hash.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let hash = hex_to_tree_hash(hash_hex)?;
        let binding_p2 = cat.map_or(coin.puzzle_hash, |value| value.info.p2_puzzle_hash);
        verify_fixed_delegated_puzzle_hash_for_binding(launcher_id, binding_p2, hash)?;
        return Ok(hash);
    }
    Ok(
        presplit_binding_from_coin_input(launcher_id, coin, spend_bundle)?
            .fixed_conditions_tree_hash,
    )
}

fn resolve_cat_reclaim_mode(
    vault_ctx: &mut VaultSpendContext,
    coin: Coin,
    cat: Cat,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
) -> SignerResult<OfferReclaimMode> {
    let fields = metadata.map(|value| &value.fields);
    if let Some(execution_mode) = metadata.and_then(|value| value.execution_mode) {
        return match execution_mode {
            OfferExecutionMode::Direct => Ok(OfferReclaimMode::DirectVault),
            OfferExecutionMode::PresplitNew | OfferExecutionMode::PresplitExisting => {
                Ok(OfferReclaimMode::PresplitOffer {
                    fixed_conditions_tree_hash: presplit_fixed_conditions_tree_hash(
                        vault_ctx.launcher_id,
                        coin,
                        Some(cat),
                        spend_bundle,
                        fields,
                    )?,
                })
            }
        };
    }
    if vault_ctx
        .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
        .is_some()
    {
        Ok(OfferReclaimMode::DirectVault)
    } else {
        Ok(OfferReclaimMode::PresplitOffer {
            fixed_conditions_tree_hash: presplit_fixed_conditions_tree_hash(
                vault_ctx.launcher_id,
                coin,
                Some(cat),
                spend_bundle,
                fields,
            )?,
        })
    }
}

fn offer_cancel_input_not_vault_owned(coin: Coin, launcher_id: Bytes32) -> SignerError {
    SignerError::OfferCancelInputNotVaultOwned {
        coin_id: hex::encode(coin.coin_id()),
        puzzle_hash: hex::encode(coin.puzzle_hash),
        launcher_id: hex::encode(launcher_id),
    }
}

fn is_not_vault_owned_presplit_err(err: &SignerError) -> bool {
    matches!(
        err,
        SignerError::PresplitCoinPuzzleHashMismatch
            | SignerError::OfferCancelNoSpendableInput
            | SignerError::OfferCancelPresplitBindingParseFailed { .. }
    )
}

/// Classify a cancellable maker coin as a vault-owned reclaim input.
///
/// # Errors
///
/// Returns [`SignerError::OfferCancelInputNotVaultOwned`] when the coin is not vault-owned.
pub(crate) async fn classify_cancellable_maker_input<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    backend: &C,
    spend_bundle: &SpendBundle,
    metadata: Option<&StoredOfferCancelMetadata>,
    coin: Coin,
) -> SignerResult<CancellableMakerInput> {
    if let Some(nonce) = vault_ctx.infer_nonce_for_p2_hash(coin.puzzle_hash) {
        return Ok(CancellableMakerInput::DirectVaultP2 { coin, nonce });
    }

    if let Some(cat) = resolve_cancellable_cat(backend, spend_bundle, coin, metadata).await? {
        let mode = resolve_cat_reclaim_mode(vault_ctx, coin, cat, spend_bundle, metadata)?;
        return Ok(CancellableMakerInput::VaultCat { cat, mode });
    }

    match presplit_binding_from_coin_input(vault_ctx.launcher_id, coin, spend_bundle) {
        Ok(binding) => Ok(CancellableMakerInput::PresplitVaultXch {
            coin,
            fixed_conditions_tree_hash: binding.fixed_conditions_tree_hash,
        }),
        Err(err) if is_not_vault_owned_presplit_err(&err) => Err(
            offer_cancel_input_not_vault_owned(coin, vault_ctx.launcher_id),
        ),
        Err(err) => Err(err),
    }
}
