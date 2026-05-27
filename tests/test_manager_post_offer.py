from __future__ import annotations

import json
from pathlib import Path

import pytest

import greenfloor.cli.manager as manager_mod
import greenfloor.runtime.cloud_wallet.adapter as adapter_mod
from greenfloor.asset_label_catalog import _dexie_lookup_token_for_cat_id
from greenfloor.cli.manager_setup import set_log_level
from greenfloor.cli.offer_build_post import (
    resolve_dexie_base_url,
    resolve_offer_publish_settings,
    resolve_splash_base_url,
)
from greenfloor.runtime.cloud_wallet.assets import (
    recent_market_resolved_asset_id_hints,
)
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
)


def test_resolve_dexie_base_url_by_network() -> None:
    assert resolve_dexie_base_url("mainnet", None) == "https://api.dexie.space"
    assert resolve_dexie_base_url("testnet11", None) == "https://api-testnet.dexie.space"
    assert resolve_dexie_base_url("testnet", None) == "https://api-testnet.dexie.space"


def test_resolve_splash_base_url_defaults_when_not_explicit() -> None:
    assert resolve_splash_base_url(None) == "http://john-deere.hoffmang.com:4000"


def test_dexie_lookup_token_for_cat_id_falls_back_to_v3_tickers(monkeypatch) -> None:
    target = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    calls: list[str] = []

    class _Resp:
        def __init__(self, payload: object):
            self._payload = payload

        def read(self) -> bytes:
            return json.dumps(self._payload).encode("utf-8")

        def __enter__(self):
            return self

        def __exit__(self, exc_type, exc, tb):
            _ = exc_type, exc, tb
            return False

    def _fake_urlopen(req, timeout=0):
        _ = timeout
        url = req.full_url if hasattr(req, "full_url") else str(req)
        calls.append(url)
        if url.endswith("/v1/swap/tokens"):
            return _Resp({"tokens": [{"id": "fa4a...a99d", "code": "wUSDC.b"}]})
        if url.endswith("/v3/prices/tickers"):
            return _Resp({"tickers": [{"ticker_id": f"{target}_xch", "base_currency": target}]})
        raise AssertionError(f"unexpected url: {url}")

    monkeypatch.setattr("greenfloor.adapters.dexie.urllib.request.urlopen", _fake_urlopen)
    row = _dexie_lookup_token_for_cat_id(
        canonical_cat_id_hex=target,
        network="mainnet",
    )
    assert row is not None
    assert str(row.get("ticker_id", "")).startswith(target)
    assert any(url.endswith("/v1/swap/tokens") for url in calls)
    assert any(url.endswith("/v3/prices/tickers") for url in calls)


def test_recent_market_resolved_asset_id_hints_reads_strategy_execution(tmp_path: Path) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    home_dir = tmp_path / "home"
    db_path = home_dir / "db" / "greenfloor.sqlite"
    db_path.parent.mkdir(parents=True, exist_ok=True)
    store = SqliteStore(db_path)
    try:
        store.add_audit_event(
            "strategy_offer_execution",
            {
                "market_id": "m1",
                "resolved_base_asset_id": "Asset_base",
                "resolved_quote_asset_id": "Asset_quote",
            },
            market_id="m1",
        )
    finally:
        store.close()

    base_hint, quote_hint = recent_market_resolved_asset_id_hints(
        program_home_dir=str(home_dir),
        market_id="m1",
    )
    assert base_hint == "Asset_base"
    assert quote_hint == "Asset_quote"


def test_format_json_output_pretty_mode_has_indentation() -> None:
    original = adapter_mod._JSON_OUTPUT_COMPACT
    try:
        adapter_mod._JSON_OUTPUT_COMPACT = False
        output = adapter_mod.format_json_output({"alpha": 1, "beta": {"gamma": 2}})
    finally:
        adapter_mod._JSON_OUTPUT_COMPACT = original
    assert output.startswith("{\n")
    assert '\n  "alpha": 1' in output


def test_format_json_output_compact_mode_is_single_line() -> None:
    original = adapter_mod._JSON_OUTPUT_COMPACT
    try:
        adapter_mod._JSON_OUTPUT_COMPACT = True
        output = adapter_mod.format_json_output({"alpha": 1, "beta": {"gamma": 2}})
    finally:
        adapter_mod._JSON_OUTPUT_COMPACT = original
    assert output == '{"alpha":1,"beta":{"gamma":2}}'


def test_resolve_offer_publish_settings_from_program(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path, provider="splash")
    venue, dexie_base, splash_base = resolve_offer_publish_settings(
        program_path=program,
        network="mainnet",
        venue_override=None,
        dexie_base_url=None,
        splash_base_url=None,
    )
    assert venue == "splash"
    assert dexie_base == "https://api.dexie.space"
    assert splash_base == "http://localhost:4000"


def test_set_log_level_updates_program_yaml(tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    code = set_log_level(program_path=program, log_level="warning")
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["updated"] is True
    assert payload["previous_log_level"] == "INFO"
    assert payload["log_level"] == "WARNING"
    assert "log_level: WARNING" in program.read_text(encoding="utf-8")


def test_main_dispatches_set_log_level_command(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    captured: dict[str, object] = {}

    def _fake_set_log_level(*, program_path: Path, log_level: str) -> int:
        captured["program_path"] = program_path
        captured["log_level"] = log_level
        return 0

    monkeypatch.setattr("greenfloor.cli.manager.set_log_level", _fake_set_log_level)
    monkeypatch.setattr(
        "sys.argv",
        [
            "greenfloor-manager",
            "--program-config",
            str(program),
            "set-log-level",
            "--log-level",
            "ERROR",
        ],
    )
    with pytest.raises(SystemExit) as exc:
        manager_mod.main()
    assert exc.value.code == 0
    assert captured["program_path"] == program
    assert captured["log_level"] == "ERROR"


def test_main_dispatches_coin_status_command(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    captured: dict[str, object] = {}

    def _fake_coin_status(**kwargs) -> int:
        captured.update(kwargs)
        return 0

    monkeypatch.setattr("greenfloor.cli.manager.coin_status", _fake_coin_status)
    monkeypatch.setattr(
        "sys.argv",
        [
            "greenfloor-manager",
            "--program-config",
            str(program),
            "coin-status",
            "--asset",
            "xch",
        ],
    )
    with pytest.raises(SystemExit) as exc:
        manager_mod.main()
    assert exc.value.code == 0
    assert captured["program_path"] == program
    assert captured["asset"] == "xch"
