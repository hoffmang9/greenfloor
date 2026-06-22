use std::fs;
use std::path::{Path, PathBuf};

use chia_protocol::{Coin, CoinSpend};
use chia_sdk_types::{run_puzzle, Condition, Conditions};
use clvm_traits::FromClvm;
use clvmr::serde::node_from_bytes;
use clvmr::{Allocator, NodePtr};
use greenfloor_engine::coinset;
use greenfloor_engine::error::SignerResult;
use greenfloor_engine::hex::{hex_to_bytes32, normalize_hex_id};
use serde_json::Value;

fn load_case_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(root)
        .expect("read replay cases dir")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}

fn coin_from_case_fields(parent_coin_id: &str, puzzle_hash: &str, amount: u64) -> Coin {
    let parent = hex_to_bytes32(&normalize_hex_id(parent_coin_id)).expect("parent coin id");
    let puzzle = hex_to_bytes32(&normalize_hex_id(puzzle_hash)).expect("puzzle hash");
    Coin::new(parent, puzzle, amount)
}

fn bytes_from_hex_field(value: &str) -> Vec<u8> {
    let mut raw = value.trim().to_ascii_lowercase();
    if raw.starts_with("0x") {
        raw = raw[2..].to_string();
    }
    if raw.len() % 2 == 1 {
        raw = format!("0{raw}");
    }
    hex::decode(raw).expect("valid hex in replay fixture")
}

fn parent_spend_creates_child(
    parent_coin: Coin,
    parent_spend: &CoinSpend,
    child_coin: Coin,
) -> SignerResult<bool> {
    let mut allocator = Allocator::new();
    let puzzle = node_from_bytes(&mut allocator, parent_spend.puzzle_reveal.as_ref())
        .map_err(|err| greenfloor_engine::error::SignerError::Driver(err.to_string()))?;
    let solution = node_from_bytes(&mut allocator, parent_spend.solution.as_ref())
        .map_err(|err| greenfloor_engine::error::SignerError::Driver(err.to_string()))?;
    let output = run_puzzle(&mut allocator, puzzle, solution)
        .map_err(|err| greenfloor_engine::error::SignerError::Driver(err.to_string()))?;
    let conditions = Conditions::<NodePtr>::from_clvm(&allocator, output)
        .map_err(|err| greenfloor_engine::error::SignerError::Driver(err.to_string()))?;
    for condition in conditions.iter() {
        if let Condition::CreateCoin(create) = condition {
            let created = Coin::new(parent_coin.coin_id(), create.puzzle_hash, create.amount);
            if created.coin_id() == child_coin.coin_id() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[test]
fn replay_captured_cat_parse_cases() {
    let raw = std::env::var("GREENFLOOR_CAT_PARSE_REPLAY_CASES_DIR").unwrap_or_default();
    if raw.trim().is_empty() {
        return;
    }
    let root = Path::new(raw.trim());
    if !root.is_dir() {
        return;
    }

    let mut failures = Vec::new();
    for case_path in load_case_paths(root) {
        let case_name = case_path.file_name().unwrap().to_string_lossy();
        let case: Value =
            serde_json::from_str(&fs::read_to_string(&case_path).expect("read case file"))
                .expect("parse case json");
        let parent_coin = coin_from_case_fields(
            case.get("parent_coin_parent_coin_id")
                .and_then(Value::as_str)
                .expect("parent_coin_parent_coin_id"),
            case.get("parent_coin_puzzle_hash")
                .and_then(Value::as_str)
                .expect("parent_coin_puzzle_hash"),
            case.get("parent_coin_amount")
                .and_then(Value::as_u64)
                .expect("parent_coin_amount"),
        );
        let child_coin = coin_from_case_fields(
            case.get("coin_parent_coin_id")
                .and_then(Value::as_str)
                .expect("coin_parent_coin_id"),
            case.get("coin_puzzle_hash")
                .and_then(Value::as_str)
                .expect("coin_puzzle_hash"),
            case.get("coin_amount")
                .and_then(Value::as_u64)
                .expect("coin_amount"),
        );
        let puzzle_reveal = bytes_from_hex_field(
            case.get("puzzle_reveal")
                .and_then(Value::as_str)
                .expect("puzzle_reveal"),
        );
        let solution = bytes_from_hex_field(
            case.get("solution")
                .and_then(Value::as_str)
                .expect("solution"),
        );
        let parent_spend = CoinSpend::new(parent_coin, puzzle_reveal.into(), solution.into());

        if !parent_spend_creates_child(parent_coin, &parent_spend, child_coin)
            .expect("evaluate parent spend conditions")
        {
            failures.push(format!(
                "{case_name}: parent spend does not recreate target child coin"
            ));
            continue;
        }

        let parsed = coinset::cat_from_parent_spend(child_coin, &parent_spend)
            .expect("parse cat from parent spend");
        if parsed.is_none() {
            failures.push(format!(
                "{case_name}: parse_cat_from_parent_spend returned None"
            ));
            continue;
        }
        let parsed = parsed.expect("checked Some");
        let parsed_id = normalize_hex_id(&hex::encode(parsed.coin.coin_id()));
        let expected_id = normalize_hex_id(&hex::encode(child_coin.coin_id()));
        if parsed_id != expected_id {
            failures.push(format!(
                "{case_name}: parsed coin id {parsed_id} != expected {expected_id}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "captured CAT parse replay failures:\n{}",
        failures.join("\n")
    );
}
