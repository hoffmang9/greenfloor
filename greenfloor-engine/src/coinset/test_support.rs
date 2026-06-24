//! Shared coinset test fixtures (not production paths).

use chia_protocol::{Bytes32, Coin, CoinSpend};
use chia_sdk_driver::{Cat, CatInfo};
use serde_json::json;

/// Test puzzle hash byte derived from amount; truncates above 255 by design for fixtures.
#[allow(clippy::cast_possible_truncation)]
pub fn puzzle_byte_from_amount(amount: u64) -> u8 {
    amount as u8
}

pub fn cat_with_amount(amount: u64) -> Cat {
    Cat::new(
        Coin::new(
            Bytes32::new([puzzle_byte_from_amount(amount); 32]),
            Bytes32::default(),
            amount,
        ),
        None,
        CatInfo::new(Bytes32::new([0x01; 32]), None, Bytes32::default()),
    )
}

fn coin_json(coin: &Coin) -> serde_json::Value {
    json!({
        "parent_coin_info": hex::encode(coin.parent_coin_info),
        "puzzle_hash": hex::encode(coin.puzzle_hash),
        "amount": coin.amount,
    })
}

fn coin_record_json(coin: &Coin, spent: bool, spent_block_index: u32) -> serde_json::Value {
    json!({
        "coin": coin_json(coin),
        "coinbase": false,
        "confirmed_block_index": 1,
        "spent": spent,
        "spent_block_index": spent_block_index,
        "timestamp": 1,
    })
}

pub fn mock_get_coin_records_by_puzzle_hash_body(coins: &[Coin]) -> String {
    json!({
        "success": true,
        "coin_records": coins
            .iter()
            .map(|coin| coin_record_json(coin, false, 0))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

pub fn mock_get_coin_record_by_name_body(parent_coin: &Coin, spent_block_index: u32) -> String {
    json!({
        "success": true,
        "coin_record": coin_record_json(parent_coin, true, spent_block_index),
    })
    .to_string()
}

pub fn mock_unspent_coin_record_by_name_body(coin: &Coin) -> String {
    json!({
        "success": true,
        "coin_record": coin_record_json(coin, false, 0),
    })
    .to_string()
}

pub fn coin_record_by_name_request_json(coin_id: Bytes32) -> serde_json::Value {
    json!({
        "name": format!("0x{}", hex::encode(coin_id.to_bytes())),
    })
}

pub fn mock_get_puzzle_and_solution_body(spend: &CoinSpend) -> String {
    json!({
        "success": true,
        "coin_solution": {
            "coin": coin_json(&spend.coin),
            "puzzle_reveal": hex::encode(spend.puzzle_reveal.as_ref()),
            "solution": hex::encode(spend.solution.as_ref()),
        },
    })
    .to_string()
}
