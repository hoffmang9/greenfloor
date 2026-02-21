from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.manager import _metrics_export
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


def test_metrics_export_aggregates_counts_latency_and_error_rates(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program)
    store = SqliteStore(db_path)
    try:
        store.add_audit_event(
            "daemon_cycle_summary",
            {"duration_ms": 100, "error_count": 0},
        )
        store.add_audit_event(
            "daemon_cycle_summary",
            {"duration_ms": 200, "error_count": 2},
        )
        store.add_audit_event(
            "strategy_offer_execution",
            {"planned_count": 4, "executed_count": 3, "items": []},
            market_id="m1",
        )
        store.add_audit_event(
            "offer_cancel_policy",
            {"triggered": True, "planned_count": 2, "executed_count": 1, "items": []},
            market_id="m1",
        )
        store.add_audit_event("dexie_offers_error", {"error": "down"}, market_id="m1")
        store.add_audit_event("xch_price_error", {"error": "timeout"})
    finally:
        store.close()

    code = _metrics_export(program_path=program, state_db=str(db_path), limit=100)
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    metrics = payload["metrics"]
    assert metrics["daemon"]["cycle_count"] == 2
    assert metrics["daemon"]["avg_cycle_duration_ms"] == 150
    assert metrics["daemon"]["cycle_error_rate"] == 0.5
    assert metrics["offer_execution"]["planned_total"] == 4
    assert metrics["offer_execution"]["executed_total"] == 3
    assert metrics["offer_execution"]["success_rate"] == 0.75
    assert metrics["cancel_policy"]["triggered_count"] == 1
    assert metrics["cancel_policy"]["planned_total"] == 2
    assert metrics["cancel_policy"]["executed_total"] == 1
    assert metrics["errors"]["event_count"] == 2
    assert metrics["errors"]["by_type"]["dexie_offers_error"] == 1
