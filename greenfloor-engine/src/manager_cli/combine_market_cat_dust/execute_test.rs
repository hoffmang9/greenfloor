use super::combine_test_support::{
    dust_combine_batch_from_ids, test_combine_batch_executor,
    test_combine_batch_executor_with_asset,
};
use super::execute::BatchPlanRunner;
use crate::coinset::test_support::{
    coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
    mock_unspent_coin_record_by_name_body,
};
use crate::coinset::CoinSpentVerifyConfig;
use crate::error::SignerError;
use crate::vault_coinset_scan::{DustCombineBatch, ProvenDustCoin};

#[tokio::test]
async fn combine_batch_executor_rejects_zero_total_batch() {
    let mut cat = crate::coinset::test_support::cat_with_amount(0);
    cat.coin = chia_protocol::Coin::new(
        crate::hex::hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
        cat.coin.puzzle_hash,
        0,
    );
    let err = test_combine_batch_executor("http://coinset.test", CoinSpentVerifyConfig::default())
        .combine_batch(&DustCombineBatch {
            items: vec![ProvenDustCoin::from_cat(cat)],
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("dust batch total is zero"));
}

#[tokio::test]
async fn combine_batch_executor_rejects_invalid_cat_asset_id() {
    let err = test_combine_batch_executor_with_asset(
        "http://coinset.test",
        "not-valid-hex",
        CoinSpentVerifyConfig::default(),
    )
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

    test_combine_batch_executor(&server.url(), CoinSpentVerifyConfig::unit_test())
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
        .expect_at_least(1)
        .create_async()
        .await;

    let err = test_combine_batch_executor(&server.url(), CoinSpentVerifyConfig::unit_test())
        .wait_for_batch_spent(&batch)
        .await
        .expect_err("verify timeout");
    assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
}
