from __future__ import annotations

import threading
from pathlib import Path

import pytest

from greenfloor.daemon.main import _run_loop, run_once
from greenfloor.storage.sqlite import SqliteStore
from tests.logging_helpers import reset_concurrent_log_handlers


def _write_program(path: Path, home_dir: Path, *, parallel_markets: bool = False) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{str(home_dir)}"',
                "  log_level: INFO",
                "runtime:",
                "  loop_interval_seconds: 30",
                "  dry_run: false",
                f"  parallel_markets: {'true' if parallel_markets else 'false'}",
                "chain_signals:",
                "  tx_block_trigger:",
                '    mode: "websocket"',
                '    websocket_url: "wss://coinset.org/ws"',
                "    websocket_reconnect_interval_seconds: 1",
                "    fallback_poll_interval_seconds: 1",
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


def _write_program_without_log_level(path: Path, home_dir: Path) -> None:
    _write_program(path, home_dir)
    text = path.read_text(encoding="utf-8")
    path.write_text(text.replace("  log_level: INFO\n", ""), encoding="utf-8")


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


def _write_markets_two(path: Path) -> None:
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
                "  - id: m2",
                "    enabled: true",
                '    base_asset: "asset2"',
                '    base_symbol: "AS2"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
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


def test_run_loop_starts_coinset_websocket_client(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home)
    _write_markets(markets)
    reset_concurrent_log_handlers(module=daemon_mod)

    calls: dict[str, int] = {"start": 0, "stop": 0, "run_once": 0}
    run_once_kwargs: dict[str, object] = {}

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            calls["start"] += 1

        def stop(self, **_kwargs) -> None:
            calls["stop"] += 1

    def _fake_run_once(**kwargs):
        calls["run_once"] += 1
        run_once_kwargs.update(kwargs)
        raise KeyboardInterrupt

    monkeypatch.setattr("greenfloor.daemon.main.CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr("greenfloor.daemon.main.run_once", _fake_run_once)

    code = _run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert calls["start"] == 1
    assert calls["stop"] == 1
    assert calls["run_once"] == 1
    assert run_once_kwargs["poll_coinset_mempool"] is False
    log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "daemon_starting mode=loop" in log_text
    assert "daemon_stopped mode=loop" in log_text


def test_run_once_parallel_markets_overlap_execution(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home, parallel_markets=True)
    _write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    started: list[str] = []
    started_lock = threading.Lock()
    both_started = threading.Event()

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        with started_lock:
            started.append(str(market.market_id))
            if len(started) == 2:
                both_started.set()
        assert both_started.wait(timeout=1.0)
        return daemon_mod._MarketCycleResult()

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.main._process_single_market_with_store", _fake_process_single_market
    )

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0
    assert set(started) == {"m1", "m2"}


def test_run_once_parallel_market_failure_isolated(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home, parallel_markets=True)
    _write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        if str(market.market_id) == "m1":
            raise RuntimeError("boom")
        return daemon_mod._MarketCycleResult(strategy_planned=2, strategy_executed=1)

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.main._process_single_market_with_store", _fake_process_single_market
    )

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=1)
    finally:
        store.close()
    assert len(events) == 1
    payload = events[0]["payload"]
    assert payload["markets_attempted"] == 2
    assert payload["markets_processed"] == 1
    assert payload["error_count"] >= 1


def test_run_once_sequential_market_failure_isolated(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home, parallel_markets=False)
    _write_markets_two(markets)
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        if str(market.market_id) == "m1":
            raise RuntimeError("boom-sequential")
        return daemon_mod._MarketCycleResult(strategy_planned=2, strategy_executed=1)

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.main._process_single_market", _fake_process_single_market
    )

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=1)
    finally:
        store.close()
    assert len(events) == 1
    payload = events[0]["payload"]
    assert payload["markets_attempted"] == 2
    assert payload["markets_processed"] == 1
    assert payload["error_count"] >= 1


def test_run_once_parallel_picks_up_new_market_next_cycle(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home, parallel_markets=True)
    _write_markets(markets)  # first cycle has one enabled market
    db_path = tmp_path / "state.sqlite"

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        pass

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    class _FakeSplashAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

    sequential_seen: list[str] = []
    parallel_seen: list[str] = []

    def _fake_process_single_market(**kwargs):
        market = kwargs["market"]
        sequential_seen.append(str(market.market_id))
        return daemon_mod._MarketCycleResult()

    def _fake_process_single_market_with_store(**kwargs):
        market = kwargs["market"]
        parallel_seen.append(str(market.market_id))
        return daemon_mod._MarketCycleResult()

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.SplashAdapter", _FakeSplashAdapter)
    monkeypatch.setattr(
        "greenfloor.daemon.main._process_single_market", _fake_process_single_market
    )
    monkeypatch.setattr(
        "greenfloor.daemon.main._process_single_market_with_store",
        _fake_process_single_market_with_store,
    )

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0
    assert sequential_seen == ["m1"]
    assert parallel_seen == []
    cycle1_parallel_count = len(parallel_seen)

    _write_markets_two(markets)  # add a new enabled market while daemon keeps running
    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(db_path),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
    )
    assert code == 0
    assert len(parallel_seen) == cycle1_parallel_count + 2
    assert set(parallel_seen[cycle1_parallel_count:]) == {"m1", "m2"}

    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(event_types=["daemon_cycle_summary"], limit=2)
    finally:
        store.close()
    attempted = sorted(int(e["payload"]["markets_attempted"]) for e in events)
    processed = sorted(int(e["payload"]["markets_processed"]) for e in events)
    assert attempted == [1, 2]
    assert processed == [1, 2]


def test_run_once_uses_websocket_capture_when_enabled(monkeypatch, tmp_path: Path) -> None:
    home = tmp_path / "home"
    state_dir = home / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home)
    _write_markets(markets)

    class _FakePriceAdapter:
        async def get_xch_price(self) -> float:
            return 30.0

    class _FakeWalletAdapter:
        def list_asset_coins_base_units(self, **_kwargs) -> list[int]:
            return []

        def execute_coin_ops(self, **_kwargs) -> dict:
            return {"dry_run": False, "planned_count": 0, "executed_count": 0, "items": []}

    class _FakeDexieAdapter:
        def __init__(self, _base_url: str) -> None:
            pass

        def get_offers(self, _offered: str, _requested: str) -> list[dict]:
            return []

    capture_calls = {"n": 0}

    def _fake_capture(**_kwargs) -> None:
        capture_calls["n"] += 1

    monkeypatch.setattr("greenfloor.daemon.main.PriceAdapter", _FakePriceAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.WalletAdapter", _FakeWalletAdapter)
    monkeypatch.setattr("greenfloor.daemon.main.DexieAdapter", _FakeDexieAdapter)
    monkeypatch.setattr("greenfloor.daemon.main._run_coinset_signal_capture_once", _fake_capture)

    code = run_once(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=state_dir,
        poll_coinset_mempool=False,
        use_websocket_capture=True,
    )
    assert code == 0
    assert capture_calls["n"] == 1


def test_run_loop_refreshes_log_level_without_restart(monkeypatch, tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home)
    _write_markets(markets)

    calls: dict[str, int] = {"run_once": 0}
    seen_levels: list[str] = []

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_initialize(home_dir: str, *, log_level: str | None) -> None:
        _ = home_dir
        seen_levels.append(str(log_level or ""))

    def _fake_run_once(**_kwargs):
        calls["run_once"] += 1
        if calls["run_once"] == 1:
            text = program.read_text(encoding="utf-8")
            program.write_text(
                text.replace("  log_level: INFO", "  log_level: WARNING"), encoding="utf-8"
            )
            return 0
        raise KeyboardInterrupt

    monkeypatch.setattr("greenfloor.daemon.main.CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr("greenfloor.daemon.main._initialize_daemon_file_logging", _fake_initialize)
    monkeypatch.setattr("greenfloor.daemon.main.run_once", _fake_run_once)
    monkeypatch.setattr("greenfloor.daemon.main.time.sleep", lambda _seconds: None)

    code = _run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert calls["run_once"] == 2
    assert seen_levels[:3] == ["INFO", "INFO", "WARNING"]


def test_run_loop_logs_when_missing_log_level_is_auto_healed(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program_without_log_level(program, home)
    _write_markets(markets)
    reset_concurrent_log_handlers(module=daemon_mod)

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_run_once(**_kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr("greenfloor.daemon.main.CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr("greenfloor.daemon.main.run_once", _fake_run_once)

    code = _run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )
    assert code == 0
    assert "log_level: INFO" in program.read_text(encoding="utf-8")
    log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "program config missing app.log_level; wrote default INFO" in log_text


def test_run_loop_orders_reload_marker_log_sleep_then_reload(monkeypatch, tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home)
    _write_markets(markets)

    sequence: list[str] = []
    real_load_program_config = daemon_mod.load_program_config
    load_calls = {"count": 0}

    class _FakeWsClient:
        def __init__(self, **_kwargs) -> None:
            pass

        def start(self) -> None:
            return

        def stop(self, **_kwargs) -> None:
            return

    def _fake_load_program_config(path: Path):
        load_calls["count"] += 1
        sequence.append(f"load_program:{load_calls['count']}")
        if load_calls["count"] == 1:
            return real_load_program_config(path)
        raise KeyboardInterrupt

    def _fake_run_once(**_kwargs):
        sequence.append("run_once")
        return 0

    def _fake_consume_reload_marker(_state_dir: Path) -> bool:
        sequence.append("consume_reload_marker")
        return True

    def _fake_log_daemon_event(*, level: int, payload: dict[str, object]) -> None:
        _ = level
        sequence.append(f"log_event:{payload.get('event')}")

    def _fake_sleep(_seconds: float) -> None:
        sequence.append("sleep")

    monkeypatch.setattr("greenfloor.daemon.main.CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr("greenfloor.daemon.main.load_program_config", _fake_load_program_config)
    monkeypatch.setattr("greenfloor.daemon.main.run_once", _fake_run_once)
    monkeypatch.setattr(
        "greenfloor.daemon.main._consume_reload_marker", _fake_consume_reload_marker
    )
    monkeypatch.setattr("greenfloor.daemon.main._log_daemon_event", _fake_log_daemon_event)
    monkeypatch.setattr("greenfloor.daemon.main.time.sleep", _fake_sleep)

    code = _run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert sequence[0] == "load_program:1"
    assert "run_once" in sequence
    assert "consume_reload_marker" in sequence
    assert "log_event:config_reloaded" in sequence
    assert "sleep" in sequence
    assert "load_program:2" in sequence
    assert sequence.index("run_once") < sequence.index("consume_reload_marker")
    assert sequence.index("consume_reload_marker") < sequence.index("log_event:config_reloaded")
    assert sequence.index("log_event:config_reloaded") < sequence.index("sleep")
    assert sequence.index("sleep") < sequence.index("load_program:2")


def test_run_loop_websocket_callbacks_use_callback_thread_store(
    monkeypatch, tmp_path: Path
) -> None:
    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_program(program, home)
    _write_markets(markets)

    ws_errors: list[Exception] = []

    class _ThreadBoundStore:
        def __init__(self, _db_path: str) -> None:
            self._thread_id = threading.get_ident()

        def _assert_thread(self) -> None:
            if threading.get_ident() != self._thread_id:
                raise AssertionError("cross_thread_store_use")

        def observe_mempool_tx_ids(self, _tx_ids) -> int:
            self._assert_thread()
            return 1

        def confirm_tx_ids(self, _tx_ids) -> int:
            self._assert_thread()
            return 1

        def add_audit_event(self, _event_type: str, _payload: dict) -> None:
            self._assert_thread()

        def close(self) -> None:
            return

    class _FakeWsClient:
        def __init__(self, **kwargs) -> None:
            self._kwargs = kwargs

        def start(self) -> None:
            def _invoke_callbacks() -> None:
                try:
                    self._kwargs["on_audit_event"]("coinset_ws_connected", {"ok": True})
                    self._kwargs["on_mempool_tx_ids"](["a" * 64])
                    self._kwargs["on_confirmed_tx_ids"](["b" * 64])
                except Exception as exc:  # pragma: no cover - assertion path
                    ws_errors.append(exc)

            t = threading.Thread(target=_invoke_callbacks)
            t.start()
            t.join()

        def stop(self, **_kwargs) -> None:
            return

    def _fake_run_once(**_kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr("greenfloor.daemon.main.SqliteStore", _ThreadBoundStore)
    monkeypatch.setattr("greenfloor.daemon.main.CoinsetWebsocketClient", _FakeWsClient)
    monkeypatch.setattr("greenfloor.daemon.main.run_once", _fake_run_once)

    code = _run_loop(
        program_path=program,
        markets_path=markets,
        allowed_keys=None,
        db_path_override=str(tmp_path / "state.sqlite"),
        coinset_base_url="https://api.coinset.org",
        state_dir=home / "state",
    )

    assert code == 0
    assert ws_errors == []


def test_daemon_instance_lock_rejects_second_holder(tmp_path: Path) -> None:
    import greenfloor.daemon.main as daemon_mod

    state_dir = tmp_path / "state"
    with daemon_mod._acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        with pytest.raises(RuntimeError, match="daemon_already_running"):
            with daemon_mod._acquire_daemon_instance_lock(state_dir=state_dir, mode="once"):
                pass


def test_main_once_exits_with_lock_conflict(monkeypatch, tmp_path: Path, capsys) -> None:
    import greenfloor.daemon.main as daemon_mod

    home = tmp_path / "home"
    home.mkdir(parents=True, exist_ok=True)
    program = tmp_path / "program.yaml"
    _write_program(program, home)
    reset_concurrent_log_handlers(module=daemon_mod)
    state_dir = tmp_path / "state"
    with daemon_mod._acquire_daemon_instance_lock(state_dir=state_dir, mode="loop"):
        monkeypatch.setattr(
            "sys.argv",
            [
                "greenfloord",
                "--once",
                "--program-config",
                str(program),
                "--state-dir",
                str(state_dir),
            ],
        )
        with pytest.raises(SystemExit) as exc:
            daemon_mod.main()
        assert exc.value.code == 3
        captured = capsys.readouterr()
        assert captured.out == ""
        log_text = (home / "logs" / "debug.log").read_text(encoding="utf-8")
        assert "daemon_lock_conflict" in log_text
