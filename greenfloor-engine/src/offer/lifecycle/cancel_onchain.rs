use chia_protocol::{Bytes32, SpendBundle};
use chia_sdk_driver::{Cat, CatSpend, Offer, SpendContext};
use chia_sdk_types::Conditions;
use clvm_utils::TreeHash;
use clvmr::Allocator;

use crate::adapters::DexieClient;
use crate::bech32m::{decode_address, decode_offer};
use crate::coinset::{
    client_for_config, fetch_presplit_cat_by_id, LiveCoinset, OfferCoinsetBackend,
};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::dexie_payload::DexieOfferPayload;
use crate::offer::presplit::{
    build_presplit_offer_cancel_inner_spend, vault_change_puzzle_hash,
    verify_presplit_cat_offer_binding, PresplitOfferBinding,
};
use crate::offer::types::OfferTerms;
use crate::vault::materialize::{
    append_vault_singleton_spend_for_vault, build_vault_cat_inner_spend,
};
use crate::vault::session::resolve_vault_spend_context;
use crate::vault::spend::VaultFastForwardSigner;

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainParams<'a> {
    pub offer_id: &'a str,
    pub receive_address: &'a str,
    pub signer_config: SignerConfig,
    pub dexie: &'a DexieClient,
    pub fee_mojos: u64,
}

#[derive(Debug, Clone)]
pub struct CancelOfferOnChainResult {
    pub operation_id: String,
}

/// Cancel an offer on-chain by spending an offered input coin back to vault change.
///
/// # Errors
///
/// Returns an error if the operation fails.
#[allow(clippy::too_many_lines)]
pub async fn cancel_offer_on_chain(
    params: CancelOfferOnChainParams<'_>,
) -> SignerResult<CancelOfferOnChainResult> {
    if params.fee_mojos > 0 {
        return Err(SignerError::Other(
            "offer cancel fee not supported yet".to_string(),
        ));
    }
    let offer_text = fetch_dexie_offer_text(params.dexie, params.offer_id).await?;
    let mut allocator = Allocator::new();
    let spend_bundle = decode_offer(&offer_text)?;
    let offer = Offer::from_spend_bundle(&mut allocator, &spend_bundle)?;
    let cancel_cat = select_cancel_cat(&offer)?;
    let coinset_client = client_for_config(&params.signer_config)?;
    let backend = LiveCoinset(&coinset_client);
    let cat = match fetch_presplit_cat_by_id(&coinset_client, cancel_cat.coin.coin_id()).await {
        Ok(cat) => cat,
        Err(crate::error::SignerError::PresplitCoinNotFound) => {
            return Err(crate::error::SignerError::OfferCancelInputCoinAlreadySpent);
        }
        Err(err) => return Err(err),
    };
    let mut vault_ctx = resolve_vault_spend_context(params.signer_config.clone()).await?;
    let change_puzzle_hash = vault_change_puzzle_hash(vault_ctx.launcher_id)?;
    let vault = backend
        .fetch_latest_vault(vault_ctx.launcher_id, vault_ctx.inner_puzzle_hash)
        .await?;
    let signer = VaultFastForwardSigner::from_context(&vault_ctx);
    let spend_bundle = if vault_ctx
        .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
        .is_some()
    {
        build_direct_vault_cat_cancel_spend_bundle(
            &mut vault_ctx,
            &vault,
            cat,
            change_puzzle_hash,
            move |message| {
                let signer = signer.clone();
                async move { signer.sign(message).await }
            },
        )
        .await?
    } else {
        let binding = presplit_binding_for_cancel(
            vault_ctx.launcher_id,
            &offer,
            &spend_bundle,
            params.receive_address,
        )?;
        verify_presplit_cat_offer_binding(&cat, &binding)?;
        build_presplit_cat_cancel_spend_bundle(
            &mut vault_ctx,
            cat,
            change_puzzle_hash,
            binding.fixed_conditions_tree_hash,
            &vault,
            move |message| {
                let signer = signer.clone();
                async move { signer.sign(message).await }
            },
        )
        .await?
    };
    let operation_id = backend.broadcast_spend_bundle(spend_bundle).await?;
    Ok(CancelOfferOnChainResult { operation_id })
}

async fn fetch_dexie_offer_text(dexie: &DexieClient, offer_id: &str) -> SignerResult<String> {
    let response = dexie.get_offer(offer_id).await?;
    if response.is_explicit_failure() {
        return Err(SignerError::OfferCancelDexieOfferNotFound);
    }
    let payload = DexieOfferPayload::new(response.into_value());
    payload
        .offer_file_text()
        .map(str::to_string)
        .ok_or(SignerError::OfferCancelOfferFileMissing)
}

fn select_cancel_cat(offer: &Offer) -> SignerResult<Cat> {
    for cats in offer.offered_coins().cats.values() {
        if let Some(cat) = cats.first() {
            return Ok(*cat);
        }
    }
    Err(SignerError::OfferCancelNoSpendableInput)
}

fn presplit_binding_for_cancel(
    launcher_id: Bytes32,
    offer: &Offer,
    spend_bundle: &SpendBundle,
    receive_address: &str,
) -> SignerResult<PresplitOfferBinding> {
    let offered_cat = select_cancel_cat(offer)?;
    let offered_amount = offered_cat.coin.amount;
    let requested_amount = offer
        .requested_payments()
        .amounts()
        .cats
        .values()
        .copied()
        .chain(std::iter::once(offer.requested_payments().amounts().xch))
        .find(|amount| *amount > 0)
        .ok_or(SignerError::OfferCancelNoSpendableInput)?;
    let offer_asset_id = offer
        .offered_coins()
        .cats
        .keys()
        .next()
        .ok_or(SignerError::OfferCancelNoSpendableInput)?;
    let request_asset_id = if offer.requested_payments().amounts().xch > 0 {
        "xch".to_string()
    } else {
        hex::encode(
            offer
                .requested_payments()
                .amounts()
                .cats
                .keys()
                .next()
                .ok_or(SignerError::OfferCancelNoSpendableInput)?,
        )
    };
    let expires_at = extract_expires_at_from_offer_spend(spend_bundle, offered_cat.coin.coin_id())?;
    let receive_puzzle_hash = decode_address(receive_address)?;
    let offer_nonce = Offer::nonce(vec![offered_cat.coin.coin_id()]);
    let terms = OfferTerms {
        receive_address: receive_address.to_string(),
        offer_asset_id: hex::encode(offer_asset_id),
        offer_amount: offered_amount,
        request_asset_id,
        request_amount: requested_amount,
        expires_at,
    };
    PresplitOfferBinding::plan(launcher_id, &terms, receive_puzzle_hash, offer_nonce)
}

fn extract_expires_at_from_offer_spend(
    spend_bundle: &SpendBundle,
    coin_id: Bytes32,
) -> SignerResult<Option<u64>> {
    use chia_sdk_types::{run_puzzle, Condition};
    use clvm_traits::FromClvm;

    let coin_spend = spend_bundle
        .coin_spends
        .iter()
        .find(|spend| spend.coin.coin_id() == coin_id)
        .ok_or(SignerError::OfferCancelNoSpendableInput)?;
    let mut allocator = Allocator::new();
    let puzzle = clvmr::serde::node_from_bytes(&mut allocator, coin_spend.puzzle_reveal.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let solution = clvmr::serde::node_from_bytes(&mut allocator, coin_spend.solution.as_ref())
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let output = run_puzzle(&mut allocator, puzzle, solution)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    let conditions = Conditions::<clvmr::NodePtr>::from_clvm(&allocator, output)
        .map_err(|err| SignerError::Driver(err.to_string()))?;
    for condition in conditions.iter() {
        if let Condition::AssertBeforeSecondsAbsolute(seconds) = condition {
            return Ok(Some(seconds.seconds));
        }
        if let Condition::AssertBeforeSecondsRelative(seconds) = condition {
            return Ok(Some(seconds.seconds));
        }
    }
    Ok(None)
}

async fn build_direct_vault_cat_cancel_spend_bundle<F, Fut>(
    vault_ctx: &mut crate::vault::spend::VaultSpendContext,
    vault: &chia_sdk_driver::Vault,
    cat: Cat,
    change_puzzle_hash: Bytes32,
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
    let nonce = vault_ctx
        .infer_nonce_for_p2_hash(cat.info.p2_puzzle_hash)
        .ok_or(SignerError::Driver(
            "failed to infer vault nonce for cancel cat".to_string(),
        ))?;
    let inner_spend = build_vault_cat_inner_spend(
        &mut ctx,
        delegated,
        vault_ctx,
        nonce,
        cat.info.p2_puzzle_hash.into(),
    )?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(cat, inner_spend)]).map_err(SignerError::from)?;
    append_vault_singleton_spend_for_vault(&mut ctx, vault_ctx, vault, sign_digest).await?;
    Ok(SpendBundle::new(ctx.take(), chia_bls::Signature::default()))
}

async fn build_presplit_cat_cancel_spend_bundle<F, Fut>(
    vault_ctx: &mut crate::vault::spend::VaultSpendContext,
    cat: Cat,
    change_puzzle_hash: Bytes32,
    fixed_conditions_hash: TreeHash,
    vault: &chia_sdk_driver::Vault,
    sign_digest: F,
) -> SignerResult<SpendBundle>
where
    F: FnOnce(Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = SignerResult<chia_secp::R1Signature>>,
{
    let mut ctx = SpendContext::new();
    let memos = ctx.hint(change_puzzle_hash).map_err(SignerError::from)?;
    let conditions = Conditions::new().create_coin(change_puzzle_hash, cat.coin.amount, memos);
    let cancel_delegated = ctx.delegated_spend(conditions).map_err(SignerError::from)?;
    let inner_spend = build_presplit_offer_cancel_inner_spend(
        &mut ctx,
        cancel_delegated,
        vault_ctx.launcher_id,
        fixed_conditions_hash,
    )?;
    Cat::spend_all(&mut ctx, &[CatSpend::new(cat, inner_spend)]).map_err(SignerError::from)?;
    append_vault_singleton_spend_for_vault(&mut ctx, vault_ctx, vault, sign_digest).await?;
    Ok(SpendBundle::new(ctx.take(), chia_bls::Signature::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_cancel_cat_requires_offered_cat() {
        let mut allocator = Allocator::new();
        let bundle = SpendBundle::new(vec![], chia_bls::Signature::default());
        let offer = Offer::from_spend_bundle(&mut allocator, &bundle).expect("offer");
        let err = select_cancel_cat(&offer).unwrap_err();
        assert!(matches!(err, SignerError::OfferCancelNoSpendableInput));
    }
}
