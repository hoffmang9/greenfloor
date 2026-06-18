"""Adapter tests for Python config IO helpers (policy validation is Rust-only)."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import (
    ScriptProgramFields,
    enabled_market_rows,
    load_markets_yaml_with_optional_overlay,
    load_yaml,
    run_config_validate,
)
from tests.helpers.manager_cli import parse_json_output, run_manager


def test_script_program_fields_from_example_program() -> None:
    raw = load_yaml(Path("config/program.yaml"))
    fields = ScriptProgramFields.from_raw(raw)
    assert fields.network == "mainnet"
    assert "key-main-1" in fields.signer_key_registry


def test_load_markets_yaml_with_optional_overlay_merges_enabled_rows(tmp_path: Path) -> None:
    base_path = tmp_path / "markets.yaml"
    overlay_path = tmp_path / "testnet-markets.yaml"
    base_path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: base_m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
            ]
        ),
        encoding="utf-8",
    )
    overlay_path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: testnet_m1",
                "    enabled: true",
                '    base_asset: "ta1"',
                '    base_symbol: "TA1"',
                '    quote_asset: "txch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "txch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
            ]
        ),
        encoding="utf-8",
    )
    raw = load_markets_yaml_with_optional_overlay(path=base_path, overlay_path=overlay_path)
    enabled = enabled_market_rows(raw)
    assert len(enabled) == 2
    assert {str(m.get("id")) for m in enabled} == {"base_m1", "testnet_m1"}


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
