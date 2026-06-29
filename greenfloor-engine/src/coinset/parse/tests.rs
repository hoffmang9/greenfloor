use super::record::coin_spend_from_solution_payload;
use serde_json::json;

#[test]
fn coin_records_from_payload_filters_non_objects() {
    use super::coin_records_from_payload;

    let payload = json!({
        "success": true,
        "coin_records": [{"coin": {"amount": 1}}, "bad"]
    });
    let records = coin_records_from_payload(&payload).expect("coin records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["coin"]["amount"], 1);
}

#[test]
fn coin_records_from_payload_errors_on_rpc_failure() {
    use super::coin_records_from_payload;

    let payload = json!({"success": false, "error": "invalid puzzle hash"});
    let err = coin_records_from_payload(&payload).expect_err("rpc failure");
    assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
}

#[test]
fn record_from_payload_errors_on_rpc_failure() {
    use super::record_from_payload;

    let payload = json!({"success": false, "coin_record": {"coin": {"amount": 1}}});
    let err = record_from_payload(&payload, "coin_record").expect_err("rpc failure");
    assert!(err.to_string().contains("success=false"));
}

#[test]
fn record_from_payload_returns_none_when_record_missing_on_success() {
    use super::record_from_payload;

    let payload = json!({"success": true});
    assert!(record_from_payload(&payload, "coin_record")
        .expect("success payload")
        .is_none());
}

#[test]
fn coin_id_from_record_prefers_explicit_name_field() {
    use super::coin_id_from_record;

    let name = "a".repeat(64);
    let record = json!({
        "coin": {
            "parent_coin_info": format!("0x{}", "b".repeat(64)),
            "puzzle_hash": format!("0x{}", "c".repeat(64)),
            "amount": 1,
            "name": format!("0x{name}"),
        }
    });
    assert_eq!(coin_id_from_record(&record), name);
}

#[test]
fn coin_id_from_record_computes_from_parent_puzzle_and_amount() {
    use super::coin_id_from_record;
    use chia_protocol::{Bytes32, Coin};

    let parent = Bytes32::new([0x11; 32]);
    let puzzle_hash = Bytes32::new([0x22; 32]);
    let amount = 42_u64;
    let expected = hex::encode(Coin::new(parent, puzzle_hash, amount).coin_id());
    let record = json!({
        "coin": {
            "parent_coin_info": format!("0x{}", hex::encode(parent)),
            "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
            "amount": amount,
        }
    });
    assert_eq!(coin_id_from_record(&record), expected);
}

#[test]
fn coin_id_from_record_returns_empty_when_coin_missing() {
    use super::coin_id_from_record;

    assert!(coin_id_from_record(&json!({})).is_empty());
}

#[test]
fn coin_from_record_builds_coin_from_nested_payload() {
    use super::coin_from_record;
    use chia_protocol::{Bytes32, Coin};

    let parent = Bytes32::new([0x11; 32]);
    let puzzle_hash = Bytes32::new([0x22; 32]);
    let record = json!({
        "coin": {
            "parent_coin_info": format!("0x{}", hex::encode(parent)),
            "puzzle_hash": format!("0x{}", hex::encode(puzzle_hash)),
            "amount": 99,
        }
    });
    let coin = coin_from_record(&record).expect("coin");
    assert_eq!(coin, Coin::new(parent, puzzle_hash, 99));
    assert!(coin_from_record(&json!({"coin": {"amount": 1}})).is_none());
}

#[test]
fn coin_spend_from_solution_payload_decodes_hex_fields() {
    use chia_protocol::{Bytes32, Coin};

    let parent = Coin::new(Bytes32::new([0x01; 32]), Bytes32::new([0x02; 32]), 1);
    let puzzle = "0102";
    let solution = "0304";
    let spend = coin_spend_from_solution_payload(
        parent,
        &json!({
            "puzzle_reveal": format!("0x{puzzle}"),
            "solution": solution,
        }),
    )
    .expect("spend");
    assert_eq!(hex::encode(spend.puzzle_reveal.as_ref()), puzzle);
    assert_eq!(hex::encode(spend.solution.as_ref()), solution);
}

#[test]
fn coin_spend_from_solution_payload_accepts_legacy_0x_prefix_only_inputs() {
    use chia_protocol::{Bytes32, Coin};

    let parent = Coin::new(Bytes32::new([0x01; 32]), Bytes32::new([0x02; 32]), 1);
    let puzzle = "AB01";
    let solution = "CD02";
    let spend = coin_spend_from_solution_payload(
        parent,
        &json!({
            "puzzle_reveal": format!("0X{puzzle}"),
            "solution": format!("0x{solution}"),
        }),
    )
    .expect("spend");
    assert_eq!(
        hex::encode(spend.puzzle_reveal.as_ref()),
        puzzle.to_ascii_lowercase()
    );
    assert_eq!(
        hex::encode(spend.solution.as_ref()),
        solution.to_ascii_lowercase()
    );
}

#[test]
fn coin_spend_from_solution_payload_rejects_empty_hex() {
    use chia_protocol::{Bytes32, Coin};

    let parent = Coin::new(Bytes32::new([0x01; 32]), Bytes32::new([0x02; 32]), 1);
    assert!(coin_spend_from_solution_payload(
        parent,
        &json!({"puzzle_reveal": "0x", "solution": "0102"}),
    )
    .is_none());
}
