from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

from greenfloor.cli.manager import _offers_reconcile
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.main import run_once
from greenfloor.storage.sqlite import SqliteStore


def _write_program(path: Path, home_dir: Path) -> None:
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
                '    api_base: "https://api.dexie.space"',
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


def _write_markets(path: Path) -> None:
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


def test_daemon_multi_cycle_price_shift_plan_post_cancel_and_reconcile(
    monkeypatch, tmp_path: Path
) -> None:
    import greenfloor.daemon.main as daemon_main

    daemon_main._POST_COOLDOWN_UNTIL.clear()
    daemon_main._CANCEL_COOLDOWN_UNTIL.clear()
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    db_path = tmp_path / "state.sqlite"
    _write_program(program, home)
    _write_markets(markets)

    class _FakePriceAdapter:
        prices = [30.0, 40.0]
        idx = 0

        async def get_xch_price(self) -> float:
            value = float(type(self).prices[type(self).idx])
            type(self).idx = min(type(self).idx + 1, len(type(self).prices) - 1)
            return value

    class _FakeCoinsetAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_all_mempool_tx_ids(self) -> list[str]:
            return []

    class _FakeWalletAdapter:
        def list_asset_coins_base_units(self, **_kwargs) -> list[int]:
            return []

        def execute_coin_ops(self, **_kwargs) -> dict:
            return {"dry_run": False, "planned_count": 0, "executed_count": 0, "items": []}

    class _FakeDexieAdapter:
        offers: dict[str, dict] = {}

        def __init__(self, _base_url: str) -> None:
            pass

        def get_offers(self, _offered: str, _requested: str) -> list[dict]:
            return [dict(v) for v in self.offers.values()]

        def get_offer(self, offer_id: str) -> dict:
            row = self.offers.get(offer_id)
            if row is None:
                raise RuntimeError("not_found")
            return dict(row)

        def post_offer(self, _offer: str) -> dict:
            self.offers["offer-1"] = {"id": "offer-1", "status": 0}
            return {"success": True, "id": "offer-1"}

        def cancel_offer(self, offer_id: str) -> dict:
            row = self.offers.setdefault(offer_id, {"id": offer_id, "status": 0})
            row["status"] = 3
            return {"success": True, "id": offer_id, "status": 3}

    call_counter = {"n": 0}

    def _fake_evaluate_market(*_args, **_kwargs) -> list[PlannedAction]:
        if call_counter["n"] == 0:
            call_counter["n"] += 1
            return [
                PlannedAction(
                    size=1,
                    repeat=1,
                    pair="xch",
                    expiry_unit="minutes",
                    expiry_value=65,
                    cancel_after_create=True,
                    reason="below_target",
                )
            ]
        return []

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.CoinsetAdapter", _FakeCoinsetAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.evaluate_market", _fake_evaluate_market)
    monkeypatch.setattr(
        "greenfloor.daemon.main._build_offer_for_action",
        lambda **_kwargs: {
            "status": "executed",
            "reason": "offer_builder_success",
            "offer": "offer1demo",
        },
    )
    monkeypatch.setattr(
        "greenfloor.daemon.main.utcnow",
        lambda: datetime(2026, 2, 20, 12, 0, tzinfo=UTC),
    )

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

    reconcile_code = _offers_reconcile(
        program_path=program,
        state_db=str(db_path),
        market_id="m1",
        limit=50,
        venue="dexie",
    )
    assert reconcile_code == 0

    store = SqliteStore(db_path)
    try:
        states = store.list_offer_states(market_id="m1", limit=10)
        assert len(states) == 1
        assert states[0]["offer_id"] == "offer-1"
        assert states[0]["state"] == "cancelled"
        assert states[0]["last_seen_status"] == 3

        events = store.list_recent_audit_events(
            event_types=[
                "strategy_offer_execution",
                "offer_cancel_policy",
                "offer_reconciliation",
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
        int(e["payload"].get("executed_count", 0)) == 1
        for e in by_type.get("strategy_offer_execution", [])
        if isinstance(e["payload"], dict)
    )
    assert any(
        bool(e["payload"].get("triggered", False))
        for e in by_type.get("offer_cancel_policy", [])
        if isinstance(e["payload"], dict)
    )
    assert any(
        e["payload"].get("new_state") == "cancelled"
        for e in by_type.get("offer_reconciliation", [])
        if isinstance(e["payload"], dict)
    )
    assert len(by_type.get("daemon_cycle_summary", [])) >= 1
