from __future__ import annotations

from pathlib import Path

from greenfloor.daemon.main import _run_loop, run_once
from tests.logging_helpers import reset_concurrent_log_handlers


def _write_program(path: Path, home_dir: Path) -> None:
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
