use chia_protocol::Bytes32;
use chia_puzzle_types::Memos;
use chia_puzzles::SETTLEMENT_PAYMENT_HASH;
use chia_sdk_driver::{Action, Id, Offer, Spends};
use clvmr::Allocator;

use crate::coinset::{spend_bundle_hex, OfferCoinsetBackend, SelectedCats};
use crate::error::{SignerError, SignerResult};
use crate::hex::tree_hash_to_hex;
use crate::offer::plan::{build_offer_payment_bundle, build_offer_request_conditions};
use crate::offer::presplit::{
    build_offer_from_presplit_cat, build_presplit_split_spend_bundle, vault_change_puzzle_hash,
    verify_presplit_cat_offer_binding, PresplitOfferBinding, PresplitPaymentContext,
};
use crate::offer::types::{
    CreateOfferResult, OfferArtifacts, OfferExecutionMode, OfferInput, PresplitArtifacts,
    PresplitCancelFields,
};
use crate::vault::materialize::materialize_vault_cat_finished_spends;
use crate::vault::spend::VaultSpendContext;

pub(crate) fn validate_existing_presplit_cat(
    presplit_cat: &chia_sdk_driver::Cat,
    offer_asset_id: Bytes32,
    offer_amount: u64,
) -> SignerResult<()> {
    if presplit_cat.info.asset_id != offer_asset_id {
        return Err(SignerError::PresplitCoinAssetMismatch);
    }
    if presplit_cat.coin.amount != offer_amount {
        return Err(SignerError::PresplitCoinAmountMismatch {
            coin: presplit_cat.coin.amount,
            offer: offer_amount,
        });
    }
    Ok(())
}

pub(crate) async fn resolve_presplit_cat_after_split<C: OfferCoinsetBackend>(
    coinset: &C,
    broadcast_split: bool,
    predicted_presplit_cat: chia_sdk_driver::Cat,
) -> SignerResult<chia_sdk_driver::Cat> {
    if broadcast_split {
        coinset
            .wait_for_unspent_cat(predicted_presplit_cat.coin.coin_id())
            .await
    } else {
        Ok(predicted_presplit_cat)
    }
}

pub(crate) async fn execute_presplit_new_offer<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    coinset: &C,
    input: OfferInput,
    receive_puzzle_hash: Bytes32,
    selection: SelectedCats,
    offer_nonce: Bytes32,
) -> SignerResult<CreateOfferResult> {
    let terms = input.terms();

    let OfferInput::PresplitNew {
        broadcast_split, ..
    } = &input
    else {
        return Err(SignerError::Other(
            "presplit new execution requires presplit-new input".to_string(),
        ));
    };

    let binding = PresplitOfferBinding::plan(
        vault_ctx.launcher_id,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )?;
    let change_puzzle_hash = vault_change_puzzle_hash(vault_ctx.launcher_id)?;

    let (split_spend_bundle, predicted_presplit_cat) = build_presplit_split_spend_bundle(
        vault_ctx,
        coinset,
        &selection.selected,
        change_puzzle_hash,
        binding.p2_puzzle_hash,
        terms.offer_amount,
        selection.change_amount,
    )
    .await?;
    let split_spend_bundle_hex = spend_bundle_hex(&split_spend_bundle)?;
    let split_broadcast_status = if *broadcast_split {
        Some(coinset.broadcast_spend_bundle(split_spend_bundle).await?)
    } else {
        None
    };

    let presplit_cat =
        resolve_presplit_cat_after_split(coinset, *broadcast_split, predicted_presplit_cat).await?;
    let presplit_coin_id_hex = hex::encode(presplit_cat.coin.coin_id());
    let cancel_fields = Some(PresplitCancelFields::from_presplit_build(
        presplit_coin_id_hex.clone(),
        tree_hash_to_hex(binding.fixed_conditions_tree_hash),
    ));
    let payment_ctx = PresplitPaymentContext::new(terms, receive_puzzle_hash, offer_nonce);
    let (offer, spend_bundle_hex, offer_nonce_hex) =
        build_offer_from_presplit_cat(presplit_cat, vault_ctx.launcher_id, &binding, &payment_ctx)?;

    Ok(CreateOfferResult::assembled(
        OfferExecutionMode::PresplitNew,
        OfferArtifacts {
            offer,
            spend_bundle_hex,
            offer_nonce: offer_nonce_hex,
            selected_coin_ids: selection
                .selected
                .iter()
                .map(|cat| hex::encode(cat.coin.coin_id()))
                .collect(),
        },
        PresplitArtifacts {
            split_spend_bundle_hex: Some(split_spend_bundle_hex),
            presplit_coin_id: Some(presplit_coin_id_hex),
            split_broadcast_status,
        },
        cancel_fields,
    ))
}

pub(crate) async fn execute_existing_presplit_offer<C: OfferCoinsetBackend>(
    coinset: &C,
    input: &OfferInput,
    receive_puzzle_hash: Bytes32,
    offer_asset_id: Bytes32,
    vault_ctx: &VaultSpendContext,
    offer_nonce: Bytes32,
) -> SignerResult<CreateOfferResult> {
    let terms = input.terms();
    let OfferInput::PresplitExisting {
        presplit_coin_id, ..
    } = input
    else {
        return Err(SignerError::Other(
            "presplit existing execution requires presplit-existing input".to_string(),
        ));
    };

    let presplit_cat = coinset.fetch_offer_input_cat(*presplit_coin_id).await?;
    validate_existing_presplit_cat(&presplit_cat, offer_asset_id, terms.offer_amount)?;
    let binding = PresplitOfferBinding::plan(
        vault_ctx.launcher_id,
        terms,
        receive_puzzle_hash,
        offer_nonce,
    )?;
    verify_presplit_cat_offer_binding(&presplit_cat, &binding)?;
    let presplit_coin_id_hex = hex::encode(presplit_cat.coin.coin_id());
    let cancel_fields = Some(PresplitCancelFields::from_presplit_build(
        presplit_coin_id_hex.clone(),
        tree_hash_to_hex(binding.fixed_conditions_tree_hash),
    ));
    let payment_ctx = PresplitPaymentContext::new(terms, receive_puzzle_hash, offer_nonce);
    let (offer, spend_bundle_hex, offer_nonce_hex) =
        build_offer_from_presplit_cat(presplit_cat, vault_ctx.launcher_id, &binding, &payment_ctx)?;

    Ok(CreateOfferResult::assembled(
        OfferExecutionMode::PresplitExisting,
        OfferArtifacts {
            offer,
            spend_bundle_hex,
            offer_nonce: offer_nonce_hex,
            selected_coin_ids: Vec::new(),
        },
        PresplitArtifacts {
            presplit_coin_id: Some(presplit_coin_id_hex),
            ..PresplitArtifacts::default()
        },
        cancel_fields,
    ))
}

pub(crate) async fn execute_direct_offer<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    coinset: &C,
    input: OfferInput,
    receive_puzzle_hash: Bytes32,
    offer_asset_id: Bytes32,
    selection: SelectedCats,
    offer_nonce: Bytes32,
) -> SignerResult<CreateOfferResult> {
    let terms = input.terms();

    let mut ctx = chia_sdk_driver::SpendContext::new();
    let mut spends = Spends::new(receive_puzzle_hash);
    for cat in &selection.selected {
        spends.add(*cat);
    }

    let offer_id = Id::Existing(offer_asset_id);
    let mut actions = vec![Action::send(
        offer_id,
        SETTLEMENT_PAYMENT_HASH.into(),
        terms.offer_amount,
        Memos::None,
    )];
    if selection.change_amount > 0 {
        actions.push(Action::send(
            offer_id,
            receive_puzzle_hash,
            selection.change_amount,
            Memos::None,
        ));
    }

    let spend_payments =
        build_offer_payment_bundle(&mut ctx, terms, receive_puzzle_hash, offer_nonce)?;
    spends.conditions.required = spends
        .conditions
        .required
        .extend(build_offer_request_conditions(
            &mut ctx,
            &spend_payments,
            terms.expires_at,
        )?);

    let deltas = spends
        .apply(&mut ctx, &actions)
        .map_err(SignerError::from)?;
    let finished = spends
        .prepare(&mut ctx, &deltas, chia_sdk_driver::Relation::None)
        .map_err(SignerError::from)?;

    let input_spend_bundle =
        materialize_vault_cat_finished_spends(&mut ctx, vault_ctx, coinset, finished).await?;

    let mut allocator = Allocator::new();
    let offer = Offer::from_input_spend_bundle(
        &mut allocator,
        input_spend_bundle.clone(),
        spend_payments.requested_payments,
        spend_payments.requested_asset_info,
    )
    .map_err(SignerError::from)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(SignerError::from)?;
    let offer_text = crate::bech32m::encode_offer(&offer_spend_bundle)?;
    let spend_bundle_hex = spend_bundle_hex(&offer_spend_bundle)?;

    Ok(CreateOfferResult::assembled(
        OfferExecutionMode::Direct,
        OfferArtifacts {
            offer: offer_text,
            spend_bundle_hex,
            offer_nonce: hex::encode(offer_nonce),
            selected_coin_ids: selection
                .selected
                .iter()
                .map(|cat| hex::encode(cat.coin.coin_id()))
                .collect(),
        },
        PresplitArtifacts::default(),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_protocol::{Bytes32, Coin};
    use chia_sdk_driver::{Cat, CatInfo};

    fn sample_cat(asset_id: Bytes32, amount: u64) -> Cat {
        Cat::new(
            Coin::new(Bytes32::new([0x22; 32]), Bytes32::default(), amount),
            None,
            CatInfo::new(asset_id, None, Bytes32::default()),
        )
    }

    #[test]
    fn existing_presplit_cat_rejects_asset_and_amount_mismatch() {
        let asset_id = Bytes32::new([0x01; 32]);
        let err = validate_existing_presplit_cat(
            &sample_cat(Bytes32::new([0x02; 32]), 1000),
            asset_id,
            1000,
        )
        .unwrap_err();
        assert!(matches!(err, SignerError::PresplitCoinAssetMismatch));

        let err =
            validate_existing_presplit_cat(&sample_cat(asset_id, 500), asset_id, 1000).unwrap_err();
        assert!(matches!(
            err,
            SignerError::PresplitCoinAmountMismatch {
                coin: 500,
                offer: 1000
            }
        ));
    }

    #[tokio::test]
    async fn resolve_presplit_cat_after_split_uses_predicted_cat_without_broadcast() {
        use crate::test_support::noop_coinset::EmptyOfferCoinset;

        let cat = sample_cat(Bytes32::new([0x01; 32]), 1000);
        let resolved = resolve_presplit_cat_after_split(&EmptyOfferCoinset, false, cat)
            .await
            .expect("predicted cat");
        assert_eq!(resolved.coin.amount, 1000);
    }
}
