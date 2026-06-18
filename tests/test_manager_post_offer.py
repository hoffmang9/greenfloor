from __future__ import annotations

import json
from pathlib import Path

from greenfloor.asset_label_catalog import _dexie_lookup_token_for_cat_id
from tests.helpers.manager_cli import parse_json_output, run_manager
from tests.helpers.manager_program_fixtures import write_manager_program


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


def test_json_dumps_pretty_mode_has_indentation() -> None:
    output = json.dumps({"alpha": 1, "beta": {"gamma": 2}}, indent=2)
    assert output.startswith("{\n")
    assert '\n  "alpha": 1' in output


def test_json_dumps_compact_mode_is_single_line() -> None:
    output = json.dumps({"alpha": 1, "beta": {"gamma": 2}}, separators=(",", ":"))
    assert output == '{"alpha":1,"beta":{"gamma":2}}'


def test_set_log_level_updates_program_yaml(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "set-log-level",
            "--log-level",
            "warning",
        ]
    )
    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["updated"] is True
    assert payload["previous_log_level"] == "INFO"
    assert payload["log_level"] == "WARNING"
    assert "log_level: WARNING" in program.read_text(encoding="utf-8")
