use super::{
    coin_records_from_json_endpoint,
    cursor::{coin_records_page_from_response, pagination_from_payload, CoinsetRecordsPagination},
    fetch_all_coinset_pages,
};
use crate::error::SignerError;

use chia_sdk_coinset::{CoinRecord, GetCoinRecordsResponse};
use serde_json::json;

fn sample_record(amount: u64) -> CoinRecord {
    use chia_protocol::{Bytes32, Coin};

    CoinRecord {
        coin: Coin::new(Bytes32::new([0x11; 32]), Bytes32::new([0x22; 32]), amount),
        confirmed_block_index: 1,
        spent_block_index: 0,
        spent: false,
        coinbase: false,
        timestamp: 1,
    }
}

#[tokio::test]
async fn fetch_all_coinset_pages_follows_cursor_for_typed_responses() {
    let pages = fetch_all_coinset_pages("get_coin_records_by_puzzle_hash", |cursor| async move {
        match cursor {
            None => coin_records_page_from_response(GetCoinRecordsResponse {
                coin_records: Some(vec![sample_record(1)]),
                error: None,
                success: true,
                truncated: Some(true),
                next_cursor: Some("page-2".to_string()),
            }),
            Some(cursor) if cursor == "page-2" => {
                coin_records_page_from_response(GetCoinRecordsResponse {
                    coin_records: Some(vec![sample_record(2)]),
                    error: None,
                    success: true,
                    truncated: None,
                    next_cursor: None,
                })
            }
            Some(other) => Err(SignerError::Coinset(format!("unexpected cursor {other}"))),
        }
    })
    .await
    .expect("paginated fetch");

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].coin.amount, 1);
    assert_eq!(pages[1].coin.amount, 2);
}

#[tokio::test]
async fn fetch_all_coinset_pages_errors_when_truncated_without_cursor() {
    let err = fetch_all_coinset_pages("get_coin_records_by_puzzle_hash", |_cursor| async {
        coin_records_page_from_response(GetCoinRecordsResponse {
            coin_records: Some(vec![sample_record(1)]),
            error: None,
            success: true,
            truncated: Some(true),
            next_cursor: None,
        })
    })
    .await
    .expect_err("missing cursor");
    assert!(err.to_string().contains("truncated without next_cursor"));
}

#[test]
fn pagination_from_payload_reads_truncated_and_cursor() {
    let payload = json!({
        "success": true,
        "truncated": true,
        "next_cursor": "abc",
        "coin_records": []
    });
    let pagination = pagination_from_payload(&payload);
    assert!(pagination.truncated);
    assert_eq!(pagination.next_cursor.as_deref(), Some("abc"));
}

#[test]
fn coin_records_page_from_response_allows_truncated_without_cursor() {
    let response = GetCoinRecordsResponse {
        coin_records: Some(vec![]),
        error: None,
        success: true,
        truncated: Some(true),
        next_cursor: None,
    };
    let (records, pagination) =
        coin_records_page_from_response(response).expect("single-page parse");
    assert!(records.is_empty());
    assert!(pagination.truncated);
    assert!(pagination.next_cursor.is_none());
}

#[tokio::test]
async fn coin_records_from_json_endpoint_follows_cursor() {
    let records =
        coin_records_from_json_endpoint("get_coin_records_by_puzzle_hashes", |cursor| async move {
            match cursor {
                None => Ok((
                    vec![json!({"coin": {"amount": 1}})],
                    CoinsetRecordsPagination {
                        truncated: true,
                        next_cursor: Some("page-2".to_string()),
                    },
                )),
                Some(cursor) if cursor == "page-2" => Ok((
                    vec![json!({"coin": {"amount": 2}})],
                    CoinsetRecordsPagination {
                        truncated: false,
                        next_cursor: None,
                    },
                )),
                Some(other) => Err(SignerError::Coinset(format!("unexpected cursor {other}"))),
            }
        })
        .await
        .expect("json pagination");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["coin"]["amount"], 1);
    assert_eq!(records[1]["coin"]["amount"], 2);
}
