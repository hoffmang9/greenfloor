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


# ---------------------------------------------------------------------------
# upsert_offer_state / list_offer_states
# ---------------------------------------------------------------------------


def test_upsert_offer_state_insert_and_list(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.upsert_offer_state(
            offer_id="offer-1", market_id="m1", state="open", last_seen_status=4
        )
        rows = store.list_offer_states()
        assert len(rows) == 1
        assert rows[0]["offer_id"] == "offer-1"
        assert rows[0]["market_id"] == "m1"
        assert rows[0]["state"] == "open"
        assert rows[0]["last_seen_status"] == 4
    finally:
        store.close()


def test_upsert_offer_state_updates_existing(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.upsert_offer_state(
            offer_id="offer-1", market_id="m1", state="open", last_seen_status=4
        )
        store.upsert_offer_state(
            offer_id="offer-1", market_id="m1", state="expired", last_seen_status=6
        )
        rows = store.list_offer_states()
        assert len(rows) == 1
        assert rows[0]["state"] == "expired"
        assert rows[0]["last_seen_status"] == 6
    finally:
        store.close()


def test_upsert_offer_state_null_last_seen_status(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.upsert_offer_state(
            offer_id="offer-2", market_id="m1", state="unknown", last_seen_status=None
        )
        rows = store.list_offer_states()
        assert len(rows) == 1
        assert rows[0]["last_seen_status"] is None
    finally:
        store.close()


def test_list_offer_states_filters_by_market_id(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.upsert_offer_state(offer_id="o1", market_id="m1", state="open", last_seen_status=4)
        store.upsert_offer_state(offer_id="o2", market_id="m2", state="open", last_seen_status=4)
        store.upsert_offer_state(offer_id="o3", market_id="m1", state="expired", last_seen_status=6)
        m1_rows = store.list_offer_states(market_id="m1")
        assert len(m1_rows) == 2
        assert all(r["market_id"] == "m1" for r in m1_rows)
        m2_rows = store.list_offer_states(market_id="m2")
        assert len(m2_rows) == 1
    finally:
        store.close()


def test_list_offer_states_respects_limit(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        for i in range(5):
            store.upsert_offer_state(
                offer_id=f"o{i}", market_id="m1", state="open", last_seen_status=4
            )
        assert len(store.list_offer_states(limit=3)) == 3
        assert store.list_offer_states(limit=0) == []
        assert store.list_offer_states(limit=-1) == []
    finally:
        store.close()


def test_list_offer_states_orders_by_updated_at_desc(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.upsert_offer_state(offer_id="first", market_id="m1", state="open", last_seen_status=4)
        store.upsert_offer_state(
            offer_id="second", market_id="m1", state="open", last_seen_status=4
        )
        rows = store.list_offer_states()
        assert rows[0]["offer_id"] == "second"
        assert rows[1]["offer_id"] == "first"
    finally:
        store.close()


# ---------------------------------------------------------------------------
# add_price_policy_snapshot
# ---------------------------------------------------------------------------


def test_add_price_policy_snapshot_roundtrip(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_price_policy_snapshot("m1", {"spread_bps": 100}, source="startup")
        store.add_price_policy_snapshot("m1", {"spread_bps": 200}, source="update")
        rows = store.conn.execute(
            "SELECT market_id, source, payload_json FROM price_policy_history ORDER BY id"
        ).fetchall()
        assert len(rows) == 2
        assert rows[0]["source"] == "startup"
        assert rows[1]["source"] == "update"
    finally:
        store.close()


# ---------------------------------------------------------------------------
# get_latest_xch_price_snapshot
# ---------------------------------------------------------------------------


def test_get_latest_xch_price_snapshot_none_when_empty(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        assert store.get_latest_xch_price_snapshot() is None
    finally:
        store.close()


def test_get_latest_xch_price_snapshot_returns_latest(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_audit_event("xch_price_snapshot", {"price_usd": 25.5})
        store.add_audit_event("xch_price_snapshot", {"price_usd": 30.0})
        assert store.get_latest_xch_price_snapshot() == 30.0
    finally:
        store.close()


def test_get_latest_xch_price_snapshot_ignores_non_price_events(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_audit_event("other_event", {"price_usd": 99.0})
        assert store.get_latest_xch_price_snapshot() is None
    finally:
        store.close()


def test_get_latest_xch_price_snapshot_rejects_non_positive(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_audit_event("xch_price_snapshot", {"price_usd": 0})
        assert store.get_latest_xch_price_snapshot() is None
        store.add_audit_event("xch_price_snapshot", {"price_usd": -5.0})
        assert store.get_latest_xch_price_snapshot() is None
    finally:
        store.close()


def test_get_latest_xch_price_snapshot_handles_malformed_payload(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_audit_event("xch_price_snapshot", {"no_price_key": True})
        assert store.get_latest_xch_price_snapshot() is None
    finally:
        store.close()


# ---------------------------------------------------------------------------
# add_coin_op_ledger_entry
# ---------------------------------------------------------------------------


def test_add_coin_op_ledger_entry_persists_all_fields(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="split",
            op_count=3,
            fee_mojos=500,
            status="executed",
            reason="normal",
            operation_id="op-abc",
        )
        row = store.conn.execute("SELECT * FROM coin_op_ledger ORDER BY id DESC LIMIT 1").fetchone()
        assert row["market_id"] == "m1"
        assert row["op_type"] == "split"
        assert row["op_count"] == 3
        assert row["fee_mojos"] == 500
        assert row["status"] == "executed"
        assert row["reason"] == "normal"
        assert row["operation_id"] == "op-abc"
    finally:
        store.close()


def test_add_coin_op_ledger_entry_null_operation_id(tmp_path: Path) -> None:
    store = SqliteStore(tmp_path / "gf.sqlite")
    try:
        store.add_coin_op_ledger_entry(
            market_id="m1",
            op_type="combine",
            op_count=1,
            fee_mojos=0,
            status="skipped",
            reason="dry_run",
            operation_id=None,
        )
        row = store.conn.execute("SELECT * FROM coin_op_ledger ORDER BY id DESC LIMIT 1").fetchone()
        assert row["operation_id"] is None
    finally:
        store.close()
