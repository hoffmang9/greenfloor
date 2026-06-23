use crate::common::{open_store, raw_conn};

#[test]
fn get_tx_signal_state_dedupes_and_ignores_empty_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_a = "a".repeat(64);
    let tx_b = "b".repeat(64);
    assert_eq!(
        store
            .observe_mempool_tx_ids(&[tx_a.clone(), tx_b.clone()])
            .expect("observe"),
        2
    );
    assert_eq!(
        store
            .confirm_tx_ids(std::slice::from_ref(&tx_a))
            .expect("confirm"),
        1
    );
    let state = store
        .get_tx_signal_state(&[
            tx_a.clone(),
            String::new(),
            "   ".to_string(),
            tx_a.clone(),
            tx_b.clone(),
            "c".repeat(64),
        ])
        .expect("state");
    assert_eq!(state.len(), 2);
    assert!(state[&tx_a].mempool_observed_at.is_some());
    assert!(state[&tx_a].tx_block_confirmed_at.is_some());
    assert!(state[&tx_b].mempool_observed_at.is_some());
    assert!(state[&tx_b].tx_block_confirmed_at.is_none());
}

#[test]
fn get_tx_signal_state_returns_empty_for_no_usable_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    assert!(store.get_tx_signal_state(&[]).expect("empty").is_empty());
    assert!(store
        .get_tx_signal_state(&[String::new(), "   ".to_string()])
        .expect("blank ids")
        .is_empty());
}

#[test]
fn tx_signal_state_normalizes_prefixed_tx_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_id = "a".repeat(64);
    assert_eq!(
        store
            .observe_mempool_tx_ids(&[format!("0x{tx_id}")])
            .expect("observe"),
        1
    );
    let state = store
        .get_tx_signal_state(std::slice::from_ref(&tx_id))
        .expect("state");
    assert!(state
        .get(&tx_id)
        .is_some_and(|row| row.mempool_observed_at.is_some()));
}

#[test]
fn confirm_tx_ids_updates_legacy_prefixed_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = open_store(&db_path);
    let tx_id = "e".repeat(64);
    raw_conn(&db_path)
        .execute(
            "INSERT INTO tx_signal_state (tx_id, mempool_observed_at) VALUES (?1, ?2)",
            rusqlite::params![format!("0x{tx_id}"), "2020-01-01T00:00:00Z"],
        )
        .expect("seed legacy row");
    assert_eq!(
        store
            .confirm_tx_ids(std::slice::from_ref(&tx_id))
            .expect("confirm"),
        1
    );
    let state = store
        .get_tx_signal_state(std::slice::from_ref(&tx_id))
        .expect("state");
    assert!(state
        .get(&tx_id)
        .and_then(|row| row.tx_block_confirmed_at.as_deref())
        .is_some());
}
