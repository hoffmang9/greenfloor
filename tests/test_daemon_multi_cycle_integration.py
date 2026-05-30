from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

from greenfloor.config.io import load_program_config
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.core.strategy import PlannedAction
from greenfloor.daemon.offer_dispatch.items import action_item
from greenfloor.daemon.strategy_execution import StrategyActionResult
from greenfloor.daemon.testing import (
    CANCEL_COOLDOWN_UNTIL,
    POST_COOLDOWN_UNTIL,
    main,
    run_once,
)
from greenfloor.runtime.offer_reconciliation import reconcile_offers
from greenfloor.storage.sqlite import SqliteStore


def write_program(path: Path, home_dir: Path) -> None:
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
                "signer:",
                '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"',
                '  kms_region: "us-west-2"',
                '  kms_public_key_hex: "02abc123"',
                "vault:",
                '  launcher_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"',
                "  custody_threshold: 1",
                "  recovery_threshold: 1",
                "  recovery_clawback_timelock: 3600",
                "  custody_keys:",
                '    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"',
                "      curve: SECP256R1",
                "  recovery_keys:",
                '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                "      curve: BLS12_381",
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


def test_daemon_multi_cycle_price_shift_plan_post_cancel_and_reconcile(
    monkeypatch, tmp_path: Path
) -> None:
    POST_COOLDOWN_UNTIL.clear()
    CANCEL_COOLDOWN_UNTIL.clear()
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    db_path = tmp_path / "state.sqlite"
    write_program(program, home)
    write_markets(markets)

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
        take_tx_id = "b" * 64

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
            self.offers["offer-1"] = {"id": "offer-1", "status": 0, "tx_id": self.take_tx_id}
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

    monkeypatch.setattr(main, "PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.inventory_scan.CoinsetAdapter", _FakeCoinsetAdapter)
    monkeypatch.setattr(main, "WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr(main, "DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.runtime.offer_reconciliation.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.market_cycle.strategy_eval_phase.evaluate_market",
        _fake_evaluate_market,
    )

    def _fake_strategy_dispatch(**kwargs):
        actions = kwargs["strategy_actions"]
        store = kwargs["store"]
        market = kwargs["market"]
        items = []
        for action in actions:
            _FakeDexieAdapter.offers["offer-1"] = {
                "id": "offer-1",
                "status": 0,
                "tx_id": _FakeDexieAdapter.take_tx_id,
            }
            store.upsert_offer_state(
                offer_id="offer-1",
                market_id=str(market.market_id),
                state=OfferLifecycleState.OPEN.value,
                last_seen_status=0,
            )
            items.append(
                action_item(
                    action,
                    status="executed",
                    reason="dexie_post_success",
                    offer_id="offer-1",
                )
            )
        return StrategyActionResult.from_items(
            planned_count=len(actions),
            action_items=items,
        )

    monkeypatch.setattr(
        "greenfloor.daemon.market_cycle.strategy_exec_phase.execute_strategy_dispatch",
        _fake_strategy_dispatch,
    )
    monkeypatch.setattr(
        main,
        "utcnow",
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
    interim_store = SqliteStore(db_path)
    try:
        _FakeDexieAdapter.offers["offer-1"] = {
            "id": "offer-1",
            "status": 0,
            "tx_id": _FakeDexieAdapter.take_tx_id,
        }
        interim_store.upsert_offer_state(
            offer_id="offer-1",
            market_id="m1",
            state=OfferLifecycleState.OPEN.value,
            last_seen_status=0,
        )
        assert interim_store.observe_mempool_tx_ids([_FakeDexieAdapter.take_tx_id]) == 1
        assert interim_store.confirm_tx_ids([_FakeDexieAdapter.take_tx_id]) == 1
    finally:
        interim_store.close()
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

    program_cfg = load_program_config(program)
    store = SqliteStore(db_path)
    try:
        reconcile_offers(
            store=store,
            dexie_api_base=program_cfg.dexie_api_base,
            target_venue="dexie",
            market_id="m1",
            limit=50,
        )
    finally:
        store.close()

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
        e["payload"].get("new_state") == "tx_block_confirmed"
        for e in by_type.get("offer_lifecycle_transition", [])
        if isinstance(e["payload"], dict)
    )
    assert any(
        e["payload"].get("signal_source") == "coinset_webhook"
        for e in by_type.get("offer_lifecycle_transition", [])
        if isinstance(e["payload"], dict)
    )
    assert len(by_type.get("daemon_cycle_summary", [])) >= 1
