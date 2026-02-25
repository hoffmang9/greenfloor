from pathlib import Path

from greenfloor.storage.sqlite import SqliteStore, StoredAlertState


def test_sqlite_alert_state_roundtrip(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        original = store.get_alert_state("m1")
        assert original.is_low is False

        store.upsert_alert_state(StoredAlertState(market_id="m1", is_low=True, last_alert_at=None))
        got = store.get_alert_state("m1")
        assert got.is_low is True
    finally:
        store.close()


def test_sqlite_audit_insert(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        store.add_audit_event("test_event", {"ok": True}, market_id="m1")
    finally:
        store.close()


def test_sqlite_daily_fee_spent(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="split",
            op_count=1,
            fee_mojos=10,
            status="executed",
            reason="stub_executed",
            operation_id="op-1",
        )
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="combine",
            op_count=1,
            fee_mojos=99,
            status="skipped",
            reason="fee_budget_guard",
            operation_id=None,
        )
        total = store.get_daily_fee_spent_mojos_utc()
        assert total == 10
    finally:
        store.close()


def test_coin_op_budget_report(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="split",
            op_count=2,
            fee_mojos=20,
            status="executed",
            reason="stub_executed",
            operation_id="op-1",
        )
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="split",
            op_count=3,
            fee_mojos=0,
            status="planned",
            reason="dry_run",
            operation_id="dryrun-1",
        )
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="combine",
            op_count=4,
            fee_mojos=0,
            status="skipped",
            reason="fee_budget_guard",
            operation_id=None,
        )
        report = store.get_coin_op_budget_report_utc()
        assert report["spent_mojos"] == 20
        assert report["executed_ops"] == 2
        assert report["planned_ops"] == 3
        assert report["skipped_ops"] == 4
        assert report["fee_budget_skipped_ops"] == 4
    finally:
        store.close()


def test_get_tx_signal_state_dedupes_and_ignores_empty_ids(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        assert store.observe_mempool_tx_ids(["tx-a", "tx-b"]) == 2
        assert store.confirm_tx_ids(["tx-a"]) == 1
        state = store.get_tx_signal_state(["tx-a", "", "  ", "tx-a", "tx-b", "tx-missing"])
        assert set(state.keys()) == {"tx-a", "tx-b"}
        assert state["tx-a"]["mempool_observed_at"] is not None
        assert state["tx-a"]["tx_block_confirmed_at"] is not None
        assert state["tx-b"]["mempool_observed_at"] is not None
        assert state["tx-b"]["tx_block_confirmed_at"] is None
    finally:
        store.close()


def test_get_tx_signal_state_returns_empty_for_no_usable_ids(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        assert store.get_tx_signal_state([]) == {}
        assert store.get_tx_signal_state(["", "   "]) == {}
    finally:
        store.close()


def test_list_recent_audit_events_filters_by_event_type_and_market(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        store.add_audit_event("strategy_offer_execution", {"id": 1}, market_id="m1")
        store.add_audit_event("offer_reconciliation", {"id": 2}, market_id="m1")
        store.add_audit_event("offer_reconciliation", {"id": 3}, market_id="m2")
        filtered = store.list_recent_audit_events(
            event_types=["offer_reconciliation"],
            market_id="m1",
            limit=10,
        )
        assert len(filtered) == 1
        assert filtered[0]["event_type"] == "offer_reconciliation"
        assert filtered[0]["market_id"] == "m1"
        assert filtered[0]["payload"]["id"] == 2
    finally:
        store.close()


def test_list_recent_audit_events_non_positive_limit_returns_empty(tmp_path: Path) -> None:
    db = tmp_path / "greenfloor.sqlite"
    store = SqliteStore(db)
    try:
        store.add_audit_event("strategy_offer_execution", {"id": 1}, market_id="m1")
        assert store.list_recent_audit_events(limit=0) == []
        assert store.list_recent_audit_events(limit=-5) == []
    finally:
        store.close()
