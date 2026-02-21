import json
import shutil
from pathlib import Path

import yaml

from greenfloor.cli.manager import _doctor


def test_doctor_reports_ok_with_example_configs(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)

    code = _doctor(program, markets, str(tmp_path / "state.sqlite"))
    assert code == 0


def test_doctor_fails_when_enabled_market_key_missing_from_registry(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)

    program_data = yaml.safe_load(program.read_text(encoding="utf-8"))
    keys = dict(program_data.get("keys", {}))
    keys["registry"] = [
        entry for entry in keys.get("registry", []) if entry.get("key_id") != "key-main-1"
    ]
    program_data["keys"] = keys
    program.write_text(yaml.safe_dump(program_data, sort_keys=False), encoding="utf-8")

    code = _doctor(program, markets, str(tmp_path / "state.sqlite"))
    assert code == 2


def test_doctor_warns_on_invalid_runtime_override_env(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad")

    code = _doctor(program, markets, str(tmp_path / "state.sqlite"))
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    warnings = payload["warnings"]
    assert any("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS" in w for w in warnings)
    assert any("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS" in w for w in warnings)
