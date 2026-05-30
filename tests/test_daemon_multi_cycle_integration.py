from __future__ import annotations

from collections.abc import Generator
from pathlib import Path

import pytest

from greenfloor.daemon.testing import run_once
from greenfloor.storage.sqlite import SqliteStore
from tests.helpers.dexie_http_mock import DexieHttpMock
from tests.helpers.engine_binary import engine_binary_path


@pytest.fixture
def dexie_mock() -> Generator[DexieHttpMock, None, None]:
    mock = DexieHttpMock()
    mock.start()
    try:
        yield mock
    finally:
        mock.stop()


def write_program(
    path: Path, home_dir: Path, *, dexie_api_base: str = "https://api.dexie.space"
) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{str(home_dir)}"',
                "runtime:",
                "  loop_interval_seconds: 30",
                "  dry_run: false",
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: false",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: false",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                f'    api_base: "{dexie_api_base}"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                '    provider: "dexie"',
                "coin_ops:",
                "  max_operations_per_run: 0",
                "  max_daily_fee_budget_mojos: 0",
                "  split_fee_mojos: 0",
                "  combine_fee_mojos: 0",
                "keys:",
                "  registry:",
                '    - key_id: "key-main-1"',
                "      fingerprint: 123456789",
                '      network: "mainnet"',
                '      keyring_yaml_path: "~/.chia_keys/keyring.yaml"',
            ]
        ),
        encoding="utf-8",
    )


def write_markets(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "asset1"',
                '    base_symbol: "AS1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    pricing:",
                "      cancel_policy_stable_vs_unstable: true",
                "    inventory:",
                "      low_watermark_base_units: 10",
                "      bucket_counts:",
                "        1: 0",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 1",
                "          target_count: 1",
                "          split_buffer_count: 0",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )


@pytest.fixture(autouse=True)
def rust_cycle_test_env(monkeypatch) -> None:
    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "30")
    monkeypatch.setenv("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC", "1")
    monkeypatch.delenv("GREENFLOOR_TEST_FORCE_MARKET_ERROR", raising=False)
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(engine_binary_path()))


def test_daemon_multi_cycle_price_shift_cancel_and_reconcile(
    monkeypatch, tmp_path: Path, dexie_mock: DexieHttpMock
) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    db_path = tmp_path / "state.sqlite"
    write_program(program, home, dexie_api_base=dexie_mock.base_url)
    write_markets(markets)

    take_tx_id = "b" * 64
    dexie_mock.set_offers(
        {
            "offer-1": {
                "id": "offer-1",
                "status": 0,
                "tx_id": take_tx_id,
                "offered": [{"asset": "asset1", "amount": 1}],
                "requested": [{"asset": "xch", "amount": 1000}],
            }
        }
    )

    seed_store = SqliteStore(db_path)
    try:
        seed_store.upsert_offer_state(
            offer_id="offer-1",
            market_id="m1",
            state="open",
            last_seen_status=0,
        )
    finally:
        seed_store.close()

    assert (
        run_once(
            program_path=program,
            markets_path=markets,
            allowed_keys=None,
            db_path_override=str(db_path),
            coinset_base_url="http://coinset.local",
            state_dir=state_dir,
        )
        == 0
    )

    interim_store = SqliteStore(db_path)
    try:
        assert interim_store.observe_mempool_tx_ids([take_tx_id]) == 1
        assert interim_store.confirm_tx_ids([take_tx_id]) == 1
    finally:
        interim_store.close()

    monkeypatch.setenv("GREENFLOOR_XCH_PRICE_USD", "40")
    assert (
        run_once(
            program_path=program,
            markets_path=markets,
            allowed_keys=None,
            db_path_override=str(db_path),
            coinset_base_url="http://coinset.local",
            state_dir=state_dir,
        )
        == 0
    )

    store = SqliteStore(db_path)
    try:
        states = store.list_offer_states(market_id="m1", limit=10)
        assert len(states) == 1
        assert states[0]["offer_id"] == "offer-1"
        assert states[0]["state"] == "cancelled"
        assert states[0]["last_seen_status"] == 3

        events = store.list_recent_audit_events(
            event_types=[
                "offer_cancel_policy",
                "offer_lifecycle_transition",
                "daemon_cycle_summary",
            ],
            limit=30,
        )
    finally:
        store.close()

    by_type: dict[str, list[dict]] = {}
    for event in events:
        by_type.setdefault(str(event["event_type"]), []).append(event)

    assert any(
        bool(e["payload"].get("triggered", False))
        for e in by_type.get("offer_cancel_policy", [])
        if isinstance(e["payload"], dict)
    )
    assert any(
        e["payload"].get("new_state") == "tx_block_confirmed"
        for e in by_type.get("offer_lifecycle_transition", [])
        if isinstance(e["payload"], dict)
    )
    assert len(by_type.get("daemon_cycle_summary", [])) >= 2
