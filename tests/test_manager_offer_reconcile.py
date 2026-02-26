from __future__ import annotations

import json
import urllib.error
from email.message import Message
from pathlib import Path

from greenfloor.cli.manager import _offers_reconcile, _offers_status
from greenfloor.storage.sqlite import SqliteStore


def _write_program(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                '  home_dir: "~/.greenfloor"',
                "runtime:",
                "  loop_interval_seconds: 30",
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: true",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: true",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                '    api_base: "https://api.dexie.space"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                '    provider: "dexie"',
            ]
        ),
        encoding="utf-8",
    )


def test_offers_reconcile_updates_states_from_dexie(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    confirmed_tx_id = "a" * 64
    store = SqliteStore(db_path)
    try:
        store.upsert_offer_state(
            offer_id="offer-ok",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
        store.upsert_offer_state(
            offer_id="offer-missing",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
        assert store.observe_mempool_tx_ids([confirmed_tx_id]) == 1
        assert store.confirm_tx_ids([confirmed_tx_id]) == 1
    finally:
        store.close()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offer(self, offer_id: str) -> dict:
            if offer_id == "offer-ok":
                return {"id": "offer-ok", "status": 4, "tx_id": confirmed_tx_id}
            raise urllib.error.HTTPError(
                url=f"https://api.dexie.space/v1/offers/{offer_id}",
                code=404,
                msg="not found",
                hdrs=Message(),
                fp=None,
            )

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)

    code = _offers_reconcile(
        program_path=program,
        state_db=str(db_path),
        market_id=None,
        limit=20,
        venue="dexie",
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["reconciled_count"] == 2
    assert payload["changed_count"] == 2
    taker_items = [row for row in payload["items"] if row["offer_id"] == "offer-ok"]
    assert taker_items[0]["taker_signal"] == "coinset_tx_block_webhook"
    assert taker_items[0]["taker_diagnostic"] == "coinset_tx_block_confirmed"
    assert taker_items[0]["signal_source"] == "coinset_webhook"

    store = SqliteStore(db_path)
    try:
        rows = {r["offer_id"]: r for r in store.list_offer_states(limit=20)}
        events = store.list_recent_audit_events(event_types=["taker_detection"], limit=20)
    finally:
        store.close()
    assert rows["offer-ok"]["state"] == "tx_block_confirmed"
    assert rows["offer-ok"]["last_seen_status"] == 4
    assert rows["offer-missing"]["state"] == "unknown_orphaned"
    assert len(events) == 1
    assert events[0]["payload"]["signal"] == "coinset_tx_block_webhook"


def test_offers_status_reports_compact_summary(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    store = SqliteStore(db_path)
    try:
        store.upsert_offer_state(
            offer_id="a1",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
        store.upsert_offer_state(
            offer_id="a2",
            market_id="m1",
            state="tx_block_confirmed",
            last_seen_status=4,
        )
        store.add_audit_event(
            "offer_reconciliation",
            {"offer_id": "a2", "new_state": "tx_block_confirmed"},
            market_id="m1",
        )
    finally:
        store.close()

    code = _offers_status(
        program_path=program,
        state_db=str(db_path),
        market_id="m1",
        limit=20,
        events_limit=10,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["offer_count"] == 2
    assert payload["by_state"]["open"] == 1
    assert payload["by_state"]["tx_block_confirmed"] == 1
    assert len(payload["recent_events"]) == 1


def test_offers_reconcile_coinset_signal_matrix(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    tx_confirmed = "c" * 64
    tx_mempool = "d" * 64
    tx_no_signal = "e" * 64
    store = SqliteStore(db_path)
    try:
        for offer_id in [
            "offer-confirmed",
            "offer-mempool",
            "offer-no-signal",
            "offer-missing-status",
        ]:
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id="m1",
                state="open",
                last_seen_status=0,
            )
        assert store.observe_mempool_tx_ids([tx_confirmed, tx_mempool]) == 2
        assert store.confirm_tx_ids([tx_confirmed]) == 1
    finally:
        store.close()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offer(self, offer_id: str) -> dict:
            if offer_id == "offer-confirmed":
                return {"id": offer_id, "status": 0, "tx_id": tx_confirmed}
            if offer_id == "offer-mempool":
                return {"id": offer_id, "status": 0, "tx_id": tx_mempool}
            if offer_id == "offer-no-signal":
                return {"id": offer_id, "status": 0, "tx_id": tx_no_signal}
            if offer_id == "offer-missing-status":
                return {"id": offer_id}
            raise RuntimeError("unexpected_offer_id")

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    code = _offers_reconcile(
        program_path=program,
        state_db=str(db_path),
        market_id=None,
        limit=20,
        venue="dexie",
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    by_offer = {row["offer_id"]: row for row in payload["items"]}

    assert by_offer["offer-confirmed"]["new_state"] == "tx_block_confirmed"
    assert by_offer["offer-confirmed"]["reason"] == "coinset_tx_block_webhook_confirmed"
    assert by_offer["offer-confirmed"]["signal_source"] == "coinset_webhook"
    assert by_offer["offer-confirmed"]["taker_signal"] == "coinset_tx_block_webhook"

    assert by_offer["offer-mempool"]["new_state"] == "mempool_observed"
    assert by_offer["offer-mempool"]["reason"] == "coinset_mempool_observed"
    assert by_offer["offer-mempool"]["signal_source"] == "coinset_mempool"
    assert by_offer["offer-mempool"]["taker_diagnostic"] == "coinset_mempool_observed"

    assert by_offer["offer-no-signal"]["new_state"] == "mempool_observed"
    assert by_offer["offer-no-signal"]["signal_source"] == "dexie_status_fallback"
    assert by_offer["offer-no-signal"]["taker_diagnostic"] == "none"

    assert by_offer["offer-missing-status"]["new_state"] == "unknown_orphaned"
    assert by_offer["offer-missing-status"]["reason"] == "missing_status"
    assert by_offer["offer-missing-status"]["signal_source"] == "none"


def test_offers_reconcile_dexie_fallback_when_coinset_tx_ids_absent(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    store = SqliteStore(db_path)
    try:
        store.upsert_offer_state(
            offer_id="offer-status-confirmed",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
        store.upsert_offer_state(
            offer_id="offer-status-cancelled",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
    finally:
        store.close()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offer(self, offer_id: str) -> dict:
            if offer_id == "offer-status-confirmed":
                return {"id": offer_id, "status": 4}
            if offer_id == "offer-status-cancelled":
                return {"id": offer_id, "status": 3}
            raise RuntimeError("unexpected_offer_id")

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    code = _offers_reconcile(
        program_path=program,
        state_db=str(db_path),
        market_id=None,
        limit=20,
        venue="dexie",
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    by_offer = {row["offer_id"]: row for row in payload["items"]}

    assert by_offer["offer-status-confirmed"]["new_state"] == "tx_block_confirmed"
    assert by_offer["offer-status-confirmed"]["signal_source"] == "dexie_status_fallback"
    assert by_offer["offer-status-confirmed"]["taker_signal"] == "none"
    assert by_offer["offer-status-confirmed"]["taker_diagnostic"] == "dexie_status_pattern_fallback"

    assert by_offer["offer-status-cancelled"]["new_state"] == "cancelled"
    assert by_offer["offer-status-cancelled"]["signal_source"] == "dexie_status_fallback"
    assert by_offer["offer-status-cancelled"]["taker_signal"] == "none"


def test_offers_reconcile_reads_nested_dexie_offer_payload_shape(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    tx_id = "f" * 64
    store = SqliteStore(db_path)
    try:
        store.upsert_offer_state(
            offer_id="offer-nested",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
    finally:
        store.close()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offer(self, offer_id: str) -> dict:
            if offer_id != "offer-nested":
                raise RuntimeError("unexpected_offer_id")
            return {
                "success": True,
                "offer": {
                    "id": offer_id,
                    "status": 4,
                    "tx_id": tx_id,
                },
            }

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    code = _offers_reconcile(
        program_path=program,
        state_db=str(db_path),
        market_id=None,
        limit=20,
        venue="dexie",
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    by_offer = {row["offer_id"]: row for row in payload["items"]}
    assert by_offer["offer-nested"]["new_state"] == "tx_block_confirmed"
    assert by_offer["offer-nested"]["last_seen_status"] == 4
    assert by_offer["offer-nested"]["signal_source"] == "dexie_status_fallback"
