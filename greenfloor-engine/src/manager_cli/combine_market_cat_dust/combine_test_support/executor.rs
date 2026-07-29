use crate::coinset::{CoinSpentVerifyConfig, CoinsetClient};

use super::super::execute::CombineBatchExecutor;
use super::RECEIVE_ADDRESS;

pub(in crate::manager_cli::combine_market_cat_dust) const TEST_CAT_ASSET_ID: &str =
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

pub(in crate::manager_cli::combine_market_cat_dust) fn test_combine_batch_executor(
    coinset_url: &str,
    verify: CoinSpentVerifyConfig,
) -> CombineBatchExecutor {
    test_combine_batch_executor_with_asset(coinset_url, TEST_CAT_ASSET_ID, verify)
}

pub(in crate::manager_cli::combine_market_cat_dust) fn test_combine_batch_executor_with_asset(
    coinset_url: &str,
    cat_asset_id: &str,
    verify: CoinSpentVerifyConfig,
) -> CombineBatchExecutor {
    CombineBatchExecutor::new(
        crate::test_support::signer_config::test_signer_config(coinset_url),
        RECEIVE_ADDRESS.to_string(),
        cat_asset_id.to_string(),
        CoinsetClient::new(coinset_url.to_string()),
        verify,
    )
}
