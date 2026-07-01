use super::combine_test_support::{
    dust_combine_batch_from_ids, RECEIVE_ADDRESS,
};
use super::execute::CombineBatchExecutor;
use crate::coinset::test_support::{
    coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
    mock_unspent_coin_record_by_name_body,
};
use crate::coinset::{CoinSpentVerifyConfig, CoinsetClient};
use crate::error::SignerError;
use crate::vault_coinset_scan::{DustCombineBatch, ProvenDustCoin};

const TEST_CAT_ASSET_ID: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn test_executor(
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

#[tokio::test]
async fn combine_batch_executor_rejects_zero_total_batch() {
    let mut cat = crate::coinset::test_support::cat_with_amount(0);
    cat.coin = chia_protocol::Coin::new(
        crate::hex::hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
        cat.coin.puzzle_hash,
        0,
    );
    let executor = test_executor("http://127.0.0.1:1", TEST_CAT_ASSET_ID, CoinSpentVerifyConfig::default());
    let err = executor
        .combine_batch(&DustCombineBatch {
            items: vec![ProvenDustCoin::from_cat(cat)],
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("dust batch total is zero"));
}

#[tokio::test]
async fn combine_batch_executor_rejects_invalid_cat_asset_id() {
    let executor = test_executor("http://127.0.0.1:1", "not-valid-hex", CoinSpentVerifyConfig::default());
    let err = executor
        .combine_batch(&dust_combine_batch_from_ids(&[1]))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid hex"));
}

#[tokio::test]
async fn combine_batch_executor_waits_until_inputs_are_spent() {
    let batch = dust_combine_batch_from_ids(&[1, 2]);
    let mut server = mockito::Server::new_async().await;
    for item in &batch.items {
        let coin = item.cat().coin;
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(mockito::Matcher::PartialJson(
                coin_record_by_name_request_json(coin.coin_id()),
            ))
            .with_body(mock_get_coin_record_by_name_body(&coin, 100))
            .create_async()
            .await;
    }

    test_executor(
        &server.url(),
        TEST_CAT_ASSET_ID,
        CoinSpentVerifyConfig {
            timeout_seconds: 5,
            poll_seconds: 1,
        },
    )
    .wait_for_batch_spent(&batch)
    .await
    .expect("inputs spent");
}

#[tokio::test]
async fn combine_batch_executor_verify_times_out_when_inputs_stay_unspent() {
    let batch = dust_combine_batch_from_ids(&[3]);
    let coin = batch.items[0].cat().coin;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/get_coin_record_by_name")
        .match_body(mockito::Matcher::PartialJson(
            coin_record_by_name_request_json(coin.coin_id()),
        ))
        .with_body(mock_unspent_coin_record_by_name_body(&coin))
        .create_async()
        .await;

    let err = test_executor(
        &server.url(),
        TEST_CAT_ASSET_ID,
        CoinSpentVerifyConfig {
            timeout_seconds: 1,
            poll_seconds: 1,
        },
    )
    .wait_for_batch_spent(&batch)
    .await
    .expect_err("verify timeout");
    assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
}
