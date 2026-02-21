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
