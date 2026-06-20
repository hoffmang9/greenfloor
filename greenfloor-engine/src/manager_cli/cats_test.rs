use super::{run_cats_add, run_cats_delete, run_cats_list, CatsAddRequest};
use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
use serde_json::json;

fn cats_test_context(
    dir: &tempfile::TempDir,
) -> crate::manager_cli::test_support::CapturedManagerContext {
    ManagerContextBuilder::new(
        dir.path().join("unused-program.yaml"),
        dir.path().join("unused-markets.yaml"),
    )
    .cats_config(dir.path().join("cats.yaml"))
    .build_capturing()
}

fn cats_list_payload(
    ctx: &crate::manager_cli::context::ManagerContext,
    captured: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) -> serde_json::Value {
    let code = run_cats_list(ctx).expect("cats-list");
    assert_eq!(code, 0);
    pop_json(captured)
}

#[tokio::test]
async fn cats_add_manual_without_dexie_lookup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = cats_test_context(&dir);
    let code = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        ticker: None,
        name: Some("Manual CAT"),
        base_symbol: Some("MCAT"),
        ticker_id: Some("manualcat_xch"),
        pool_id: Some("pool-manual"),
        last_price_xch: Some("0.42"),
        target_usd_per_unit: Some("4.2"),
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("cats-add");
    assert_eq!(code, 0);
    let add_payload = pop_json(&harness.captured);
    assert_eq!(add_payload.get("added"), Some(&json!(true)));
    let payload = cats_list_payload(&harness.ctx, &harness.captured);
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
}

#[tokio::test]
async fn cats_add_replace_required_for_existing_asset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = cats_test_context(&dir);
    let cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let first = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some(cat_id),
        ticker: None,
        name: Some("First"),
        base_symbol: Some("ONE"),
        ticker_id: None,
        pool_id: None,
        last_price_xch: None,
        target_usd_per_unit: None,
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("first add");
    assert_eq!(first, 0);
    let _ = pop_json(&harness.captured);
    let second = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some(cat_id),
        ticker: None,
        name: Some("Second"),
        base_symbol: Some("TWO"),
        ticker_id: None,
        pool_id: None,
        last_price_xch: None,
        target_usd_per_unit: None,
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("second add");
    assert_eq!(second, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("error"), Some(&json!("cat_already_exists")));
}

#[tokio::test]
async fn cats_delete_by_cat_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = cats_test_context(&dir);
    let cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let added = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some(cat_id),
        ticker: None,
        name: Some("Delete Me"),
        base_symbol: Some("DEL"),
        ticker_id: None,
        pool_id: None,
        last_price_xch: None,
        target_usd_per_unit: None,
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("cats-add");
    assert_eq!(added, 0);
    let _ = pop_json(&harness.captured);
    let deleted = run_cats_delete(
        &harness.ctx,
        "mainnet",
        Some(cat_id),
        None,
        false,
        true,
        false,
    )
    .await
    .expect("cats-delete");
    assert_eq!(deleted, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("deleted"), Some(&json!(true)));
    assert!(cats_list_payload(&harness.ctx, &harness.captured)
        .get("cats")
        .and_then(|v| v.as_array())
        .is_some_and(std::vec::Vec::is_empty));
}

#[tokio::test]
async fn cats_delete_requires_confirmation_when_not_yes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = cats_test_context(&dir);
    let cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let added = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some(cat_id),
        ticker: None,
        name: Some("Needs Confirm"),
        base_symbol: Some("CNF"),
        ticker_id: None,
        pool_id: None,
        last_price_xch: None,
        target_usd_per_unit: None,
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("cats-add");
    assert_eq!(added, 0);
    let _ = pop_json(&harness.captured);
    let deleted = run_cats_delete(
        &harness.ctx,
        "mainnet",
        Some(cat_id),
        None,
        false,
        false,
        false,
    )
    .await
    .expect("cats-delete");
    assert_eq!(deleted, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("error"), Some(&json!("confirmation_required")));
}

#[tokio::test]
async fn cats_delete_preflight_only_does_not_delete() {
    let dir = tempfile::tempdir().expect("tempdir");
    let harness = cats_test_context(&dir);
    let cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let added = run_cats_add(CatsAddRequest {
        ctx: &harness.ctx,
        network: "mainnet",
        cat_id: Some(cat_id),
        ticker: None,
        name: Some("Preflight Only"),
        base_symbol: Some("PFL"),
        ticker_id: None,
        pool_id: None,
        last_price_xch: None,
        target_usd_per_unit: None,
        use_dexie_lookup: false,
        replace: false,
    })
    .await
    .expect("cats-add");
    assert_eq!(added, 0);
    let _ = pop_json(&harness.captured);
    let preflight = run_cats_delete(
        &harness.ctx,
        "mainnet",
        Some(cat_id),
        None,
        false,
        false,
        true,
    )
    .await
    .expect("cats-delete preflight");
    assert_eq!(preflight, 0);
    let _ = pop_json(&harness.captured);
    assert_eq!(
        cats_list_payload(&harness.ctx, &harness.captured)
            .get("cats")
            .and_then(|v| v.as_array())
            .map_or(0, std::vec::Vec::len),
        1
    );
}
