import shutil
from pathlib import Path

import yaml

from tests.helpers.manager_cli import parse_json_output, run_manager


def _run_doctor(
    *,
    program: Path,
    markets: Path,
    state_db: str,
    env: dict[str, str] | None = None,
) -> tuple[int, dict]:
    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "--state-db",
            state_db,
            "doctor",
        ],
        env=env,
    )
    return code, parse_json_output(stdout)


def test_doctor_reports_ok_with_example_configs(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)

    code, _payload = _run_doctor(
        program=program,
        markets=markets,
        state_db=str(tmp_path / "state.sqlite"),
    )
    assert code == 0


def test_doctor_fails_when_enabled_market_key_missing_from_registry(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)

    markets_data = yaml.safe_load(markets.read_text(encoding="utf-8"))
    for market in markets_data.get("markets", []):
        if market.get("enabled"):
            market["signer_key_id"] = ""
            break
    markets.write_text(yaml.safe_dump(markets_data, sort_keys=False), encoding="utf-8")

    code, payload = _run_doctor(
        program=program,
        markets=markets,
        state_db=str(tmp_path / "state.sqlite"),
    )
    assert code == 2
    assert payload["ok"] is False
    assert any("missing signer_key_id" in problem for problem in payload["problems"])


def test_doctor_warns_on_invalid_runtime_override_env(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/program.yaml", program)
    shutil.copyfile("config/markets.yaml", markets)
    monkeypatch.setenv("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0")
    monkeypatch.setenv("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad")

    code, payload = _run_doctor(
        program=program,
        markets=markets,
        state_db=str(tmp_path / "state.sqlite"),
    )
    assert code == 0
    warnings = payload["warnings"]
    assert any("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS" in w for w in warnings)
    assert any("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS" in w for w in warnings)
