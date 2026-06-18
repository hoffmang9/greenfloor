"""Adapter tests for Python config IO helpers (policy validation is Rust-only)."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import (
    enabled_market_rows,
    load_markets_fields,
    load_program_fields,
    load_yaml,
    materialize_minimal_program_template,
    run_config_validate,
)
from tests.helpers.manager_cli import parse_json_output, run_manager


def test_load_program_fields_reads_example_program(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    program.write_text(Path("config/program.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    fields = load_program_fields(program_config=program)
    assert fields.get("network") == "mainnet"
    registry = fields.get("keys_registry")
    assert isinstance(registry, dict)
    assert "key-main-1" in registry


def test_load_markets_fields_reads_example_markets() -> None:
    fields = load_markets_fields(
        markets_config=Path("config/markets.yaml"),
        testnet_markets_config=Path("config/testnet-markets.yaml"),
    )
    enabled = enabled_market_rows(fields)
    assert enabled
    assert all(bool(row.get("enabled")) for row in enabled)


def test_materialize_minimal_program_template(tmp_path: Path) -> None:
    home = tmp_path / "home"
    program = tmp_path / "program.yaml"
    materialize_minimal_program_template(
        program, home_dir=home, dexie_api_base="https://dexie.test"
    )
    raw = load_yaml(program)
    assert raw["app"]["home_dir"] == str(home)
    assert raw["venues"]["dexie"]["api_base"] == "https://dexie.test"
    assert raw["dev"]["python"]["min_version"] == "3.11"


def test_run_config_validate_subprocess_accepts_example_configs(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    program.write_text(Path("config/program.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    markets.write_text(Path("config/markets.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    code = run_config_validate(program_config=program, markets_config=markets)
    assert code == 0


def test_run_config_validate_program_only_subprocess(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    program.write_text(Path("config/program.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    code = run_config_validate(program_config=program, program_only=True)
    assert code == 0


def test_manager_config_validate_cli_emits_ok_json(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    program.write_text(Path("config/program.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    markets.write_text(Path("config/markets.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "config-validate",
        ]
    )
    payload = parse_json_output(stdout)
    assert code == 0
    assert payload.get("ok") is True


def test_manager_markets_fields_cli_emits_enabled_rows(tmp_path: Path) -> None:
    markets = tmp_path / "markets.yaml"
    markets.write_text(Path("config/markets.yaml").read_text(encoding="utf-8"), encoding="utf-8")
    code, stdout, _stderr = run_manager(
        [
            "--markets-config",
            str(markets),
            "--json",
            "markets-fields",
        ]
    )
    payload = parse_json_output(stdout)
    assert code == 0
    enabled = enabled_market_rows(payload)
    assert enabled
