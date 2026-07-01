use crate::coinset::{
    wait_until_coins_spent, CoinSpentVerifyConfig, CoinsetClient, MIN_CAT_OUTPUT_MOJOS,
};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::vault::mixed_split::{
    build_and_optionally_broadcast_vault_cat_mixed_split_with_preselected_cats, MixedSplitRequest,
    MixedSplitResult,
};
use crate::vault_coinset_scan::DustCombineBatch;

use super::batch_plan::{run_batch_plan, BatchPlanRunner, CombineBatchPlanOutcome};
use super::batches::DustBatchRunSelection;

pub(crate) struct CombineBatchExecutor {
    signer_config: SignerConfig,
    receive_address: String,
    cat_asset_id: String,
    client: CoinsetClient,
    verify: CoinSpentVerifyConfig,
}

impl CombineBatchExecutor {
    pub(crate) fn new(
        signer_config: SignerConfig,
        receive_address: String,
        cat_asset_id: String,
        client: CoinsetClient,
        verify: CoinSpentVerifyConfig,
    ) -> Self {
        Self {
            signer_config,
            receive_address,
            cat_asset_id,
            client,
            verify,
        }
    }
}

impl BatchPlanRunner for CombineBatchExecutor {
    async fn combine_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult> {
        let total = batch.total_amount();
        if total == 0 {
            return Err(SignerError::Other("dust batch total is zero".to_string()));
        }
        let coin_ids = batch.coin_ids()?;
        let request = MixedSplitRequest {
            receive_address: self.receive_address.clone(),
            asset_id: hex_to_bytes32(&self.cat_asset_id)?,
            output_amounts: vec![total],
            coin_ids,
            allow_sub_cat_output: total < MIN_CAT_OUTPUT_MOJOS,
            fee_mojos: 0,
        };
        build_and_optionally_broadcast_vault_cat_mixed_split_with_preselected_cats(
            self.signer_config.clone(),
            request,
            batch.cats(),
            true,
            &self.client,
        )
        .await
    }

    async fn wait_for_batch_spent(&self, batch: &DustCombineBatch) -> SignerResult<()> {
        let coin_ids = batch.coin_ids()?;
        wait_until_coins_spent(&self.client, &coin_ids, self.verify).await
    }
}

#[allow(clippy::large_futures)]
pub async fn execute_combine_batches(
    signer_config: &SignerConfig,
    client: &CoinsetClient,
    receive_address: &str,
    cat_asset_id: &str,
    selection: &DustBatchRunSelection<'_>,
    verify: CoinSpentVerifyConfig,
) -> CombineBatchPlanOutcome {
    run_batch_plan(
        &CombineBatchExecutor::new(
            signer_config.clone(),
            receive_address.to_string(),
            cat_asset_id.to_string(),
            client.clone(),
            verify,
        ),
        selection,
    )
    .await
}
