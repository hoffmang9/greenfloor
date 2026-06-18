use std::path::Path;

use super::fixtures::{parse_json_output, run_manager};
use serde_json::json;

fn cats_list(cats_path: &Path) -> serde_json::Value {
    let output = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-list",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    parse_json_output(&output.stdout)
}

#[test]
fn cats_add_manual_without_dexie_lookup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let output = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "--name",
            "Manual CAT",
            "--base-symbol",
            "MCAT",
            "--ticker-id",
            "manualcat_xch",
            "--pool-id",
            "pool-manual",
            "--last-price-xch",
            "0.42",
            "--target-usd-per-unit",
            "4.2",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(output.status.code(), Some(0));
    let payload = cats_list(&cats_path);
    let rows = payload
        .get("cats")
        .and_then(|v| v.as_array())
        .expect("cats");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.get("name"), Some(&json!("Manual CAT")));
    assert_eq!(row.get("base_symbol"), Some(&json!("MCAT")));
    assert_eq!(
        row.get("asset_id"),
        Some(&json!(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ))
    );
    let add_payload = parse_json_output(&output.stdout);
    assert_eq!(add_payload.get("added"), Some(&json!(true)));
}

#[test]
fn cats_add_replace_required_for_existing_asset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let first = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "First",
            "--base-symbol",
            "ONE",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(first.status.code(), Some(0));
    let second = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-add",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--name",
            "Second",
            "--base-symbol",
            "TWO",
            "--no-dexie-lookup",
        ],
        None,
        None,
    );
    assert_eq!(second.status.code(), Some(2));
    let payload = parse_json_output(&second.stdout);
    assert_eq!(payload.get("error"), Some(&json!("cat_already_exists")));
}

#[test]
fn cats_delete_by_cat_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Delete Me",
                "--base-symbol",
                "DEL",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    let deleted = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
            "--yes",
        ],
        None,
        None,
    );
    assert_eq!(deleted.status.code(), Some(0));
    let payload = parse_json_output(&deleted.stdout);
    assert_eq!(payload.get("deleted"), Some(&json!(true)));
    assert!(cats_list(&cats_path)
        .get("cats")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| rows.is_empty()));
}

#[test]
fn cats_delete_requires_confirmation_when_not_yes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Needs Confirm",
                "--base-symbol",
                "CNF",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    let deleted = run_manager(
        &[
            "--cats-config",
            cats_path.to_str().expect("cats"),
            "cats-delete",
            "--network",
            "mainnet",
            "--cat-id",
            cat_id,
        ],
        None,
        None,
    );
    assert_eq!(deleted.status.code(), Some(2));
    let payload = parse_json_output(&deleted.stdout);
    assert_eq!(payload.get("error"), Some(&json!("confirmation_required")));
}

#[test]
fn cats_delete_preflight_only_does_not_delete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cats_path = dir.path().join("cats.yaml");
    let cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-add",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--name",
                "Preflight Only",
                "--base-symbol",
                "PFL",
                "--no-dexie-lookup",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_manager(
            &[
                "--cats-config",
                cats_path.to_str().expect("cats"),
                "cats-delete",
                "--network",
                "mainnet",
                "--cat-id",
                cat_id,
                "--preflight-only",
            ],
            None,
            None,
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        cats_list(&cats_path)
            .get("cats")
            .and_then(|v| v.as_array())
            .map_or(0, |rows| rows.len()),
        1
    );
}
