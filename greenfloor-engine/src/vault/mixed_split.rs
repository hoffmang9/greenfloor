use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::Memos;
use chia_sdk_driver::{Action, Cat, Id, Relation, SpendContext, Spends};

use crate::bech32m::decode_address;
use crate::coinset::{self, LiveCoinset, OfferCoinsetBackend, MIN_CAT_OUTPUT_MOJOS};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::vault::materialize::materialize_vault_cat_finished_spends;
use crate::vault::session::resolve_vault_spend_context;
use crate::vault::spend::VaultSpendContext;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MixedSplitRequest {
    pub receive_address: String,
    #[serde(
        deserialize_with = "crate::offer::types::deserialize_bytes32",
        serialize_with = "crate::offer::types::serialize_bytes32"
    )]
    pub asset_id: Bytes32,
    pub output_amounts: Vec<u64>,
    #[serde(
        default,
        deserialize_with = "crate::offer::types::deserialize_coin_ids",
        serialize_with = "crate::offer::types::serialize_coin_ids"
    )]
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
    if request.output_amounts.contains(&0) {
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

enum CatSelection {
    FetchFromCoinset,
    Preselected(Vec<Cat>),
}

async fn build_vault_cat_mixed_split_with_selection(
    config: SignerConfig,
    request: MixedSplitRequest,
    broadcast: bool,
    selection_mode: CatSelection,
) -> SignerResult<MixedSplitResult> {
    validate_mixed_split_request(&request)?;

    let client = coinset::client_for_config(&config)?;
    let mut vault_ctx = resolve_vault_spend_context(config).await?;
    let backend = LiveCoinset(&client);
    let receive_puzzle_hash = decode_address(&request.receive_address)?;

    let target_total: u64 = request.output_amounts.iter().sum();
    let selection = match selection_mode {
        CatSelection::Preselected(cats) => {
            coinset::coin_select::finalize_preselected_cats_for_spend(
                cats,
                &request.coin_ids,
                target_total,
            )?
        }
        CatSelection::FetchFromCoinset => {
            backend
                .select_cats_for_spend(
                    &request.receive_address,
                    request.asset_id,
                    &request.coin_ids,
                    target_total,
                )
                .await?
        }
    };
    let change_amount = selection.change_amount;
    if !request.allow_sub_cat_output && change_amount > 0 && change_amount < MIN_CAT_OUTPUT_MOJOS {
        return Err(SignerError::CatChangeBelowMinimum);
    }

    let spend_bundle = build_vault_cat_mixed_split_spend_bundle(
        &mut vault_ctx,
        &backend,
        selection.selected.clone(),
        receive_puzzle_hash,
        request.asset_id,
        &request.output_amounts,
        change_amount,
    )
    .await?;

    let spend_bundle_hex = coinset::spend_bundle_hex(&spend_bundle)?;
    let broadcast_status = if broadcast {
        Some(backend.broadcast_spend_bundle(spend_bundle).await?)
    } else {
        None
    };

    Ok(MixedSplitResult {
        spend_bundle_hex,
        broadcast_status,
        selected_coin_ids: selection
            .selected
            .iter()
            .map(|cat| hex::encode(cat.coin.coin_id()))
            .collect(),
        offered_total: selection.offered_total,
        target_total,
        change_amount,
    })
}

/// Build and optionally broadcast vault cat mixed split.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_and_optionally_broadcast_vault_cat_mixed_split(
    config: SignerConfig,
    request: MixedSplitRequest,
    broadcast: bool,
) -> SignerResult<MixedSplitResult> {
    build_vault_cat_mixed_split_with_selection(
        config,
        request,
        broadcast,
        CatSelection::FetchFromCoinset,
    )
    .await
}

/// Build and optionally broadcast a vault mixed split using lineage-proven CAT inputs.
///
/// Used by dust combine after lineage preflight; callers must pass cats that match
/// `request.coin_ids` (enforced at selection time).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn build_and_optionally_broadcast_vault_cat_mixed_split_with_preselected_cats(
    config: SignerConfig,
    request: MixedSplitRequest,
    preselected_cats: Vec<Cat>,
    broadcast: bool,
) -> SignerResult<MixedSplitResult> {
    build_vault_cat_mixed_split_with_selection(
        config,
        request,
        broadcast,
        CatSelection::Preselected(preselected_cats),
    )
    .await
}

async fn build_vault_cat_mixed_split_spend_bundle<C: OfferCoinsetBackend>(
    vault_ctx: &mut VaultSpendContext,
    coinset: &C,
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

    let deltas = spends
        .apply(&mut ctx, &actions)
        .map_err(SignerError::from)?;
    let finished = spends
        .prepare(&mut ctx, &deltas, Relation::None)
        .map_err(SignerError::from)?;

    materialize_vault_cat_finished_spends(&mut ctx, vault_ctx, coinset, finished).await
}

#[cfg(test)]
mod tests {
    use super::{validate_mixed_split_request, MixedSplitRequest};
    use crate::error::SignerError;
    use chia_protocol::Bytes32;

    fn sample_request(output_amounts: Vec<u64>, allow_sub_cat_output: bool) -> MixedSplitRequest {
        MixedSplitRequest {
            receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"
                .to_string(),
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
        assert!(matches!(
            err,
            SignerError::MixedSplitVaultWithFeeNotSupported
        ));
    }

    #[test]
    fn rejects_empty_output_amounts() {
        let err = validate_mixed_split_request(&sample_request(vec![], false)).unwrap_err();
        assert!(matches!(err, SignerError::MissingOutputAmounts));
    }

    #[test]
    fn rejects_zero_output_amount() {
        let err = validate_mixed_split_request(&sample_request(vec![1000, 0], false)).unwrap_err();
        assert!(matches!(err, SignerError::InvalidOutputAmount));
    }

    #[tokio::test]
    async fn build_mixed_split_spend_bundle_materializes_via_simulator() {
        use crate::test_support::simulator::harness::SimulatorVaultHarness;
        use crate::test_support::simulator::SimulatorOfferCoinset;

        let mut harness = SimulatorVaultHarness::new();
        let cat = harness.fund_vault_cat(5_000);
        let coinset = SimulatorOfferCoinset::new(&harness.chain);
        coinset.register_cat(cat);
        let receive_puzzle_hash = harness.chain.p2_message_hash;
        let spend_bundle = super::build_vault_cat_mixed_split_spend_bundle(
            &mut harness.vault_ctx,
            &coinset,
            vec![cat],
            receive_puzzle_hash,
            harness.chain.asset_id,
            &[1_000, 2_000],
            2_000,
        )
        .await
        .expect("mixed split bundle");
        assert!(!spend_bundle.coin_spends.is_empty());
    }
}
