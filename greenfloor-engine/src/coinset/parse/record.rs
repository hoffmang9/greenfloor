use chia_protocol::{Bytes32, Coin, CoinSpend};
use serde_json::Value;

use crate::hex::{hex_to_bytes, hex_to_bytes32, normalize_hex_id};

fn normalized_hex_field(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(normalize_hex_id)
        .unwrap_or_default()
}

fn first_normalized_hex(values: [Option<&Value>; 4]) -> String {
    for value in values {
        let normalized = normalized_hex_field(value);
        if !normalized.is_empty() {
            return normalized;
        }
    }
    String::new()
}

fn coin_triple_from_value(coin: &Value) -> Option<(Bytes32, Bytes32, u64)> {
    let parent_hex = normalize_hex_id(coin.get("parent_coin_info")?.as_str()?);
    let puzzle_hex = normalize_hex_id(coin.get("puzzle_hash")?.as_str()?);
    if parent_hex.is_empty() || puzzle_hex.is_empty() {
        return None;
    }
    let parent = hex_to_bytes32(&parent_hex).ok()?;
    let puzzle_hash = hex_to_bytes32(&puzzle_hex).ok()?;
    let amount = coin.get("amount").and_then(Value::as_u64)?;
    Some((parent, puzzle_hash, amount))
}

#[must_use]
pub fn coin_id_from_record(record: &Value) -> String {
    let Some(coin) = record.get("coin") else {
        return String::new();
    };
    let explicit = first_normalized_hex([
        coin.get("name"),
        coin.get("coin_id"),
        coin.get("coin_name"),
        record.get("name"),
    ]);
    if !explicit.is_empty() {
        return explicit;
    }
    coin_triple_from_value(coin)
        .map(|(parent, puzzle_hash, amount)| {
            hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id())
        })
        .unwrap_or_default()
}

#[must_use]
pub fn coin_from_record(record: &Value) -> Option<Coin> {
    let (parent, puzzle_hash, amount) = coin_triple_from_value(record.get("coin")?)?;
    Some(Coin::new(parent, puzzle_hash, amount))
}

#[must_use]
pub fn coin_spend_from_solution_payload(parent_coin: Coin, solution: &Value) -> Option<CoinSpend> {
    let puzzle_reveal_hex = solution.get("puzzle_reveal")?.as_str()?.trim();
    let solution_hex = solution.get("solution")?.as_str()?.trim();
    if puzzle_reveal_hex.is_empty() || solution_hex.is_empty() {
        return None;
    }
    let puzzle_reveal = hex_to_bytes(puzzle_reveal_hex).ok()?;
    let solution_bytes = hex_to_bytes(solution_hex).ok()?;
    Some(CoinSpend::new(
        parent_coin,
        puzzle_reveal.into(),
        solution_bytes.into(),
    ))
}
