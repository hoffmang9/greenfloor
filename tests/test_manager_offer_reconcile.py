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
    finally:
        store.close()

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offer(self, offer_id: str) -> dict:
            if offer_id == "offer-ok":
                return {"id": "offer-ok", "status": 4}
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
    assert taker_items[0]["taker_signal"] == "canonical_offer_state_transition"
    assert taker_items[0]["taker_diagnostic"] == "dexie_status_pattern"

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
    assert events[0]["payload"]["signal"] == "canonical_offer_state_transition"


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
