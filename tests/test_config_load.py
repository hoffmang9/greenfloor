from __future__ import annotations

from pathlib import Path

import pytest
import yaml

from greenfloor.config.io import (
    enabled_market_rows,
    load_markets_yaml_with_optional_overlay,
    load_yaml,
    run_config_validate,
    ScriptProgramFields,
)
from tests.helpers.manager_cli import parse_json_output, run_manager


def test_load_program_config_smoke() -> None:
    raw = load_yaml(Path("config/program.yaml"))
    fields = ScriptProgramFields.from_raw(raw)
    dev = raw.get("dev", {})
    python = dev.get("python", {}) if isinstance(dev, dict) else {}
    notifications = raw.get("notifications", {})
    low_inventory = (
        notifications.get("low_inventory_alerts", {})
        if isinstance(notifications, dict)
        else {}
    )
    chain_signals = raw.get("chain_signals", {})
    tx_block = (
        chain_signals.get("tx_block_trigger", {})
        if isinstance(chain_signals, dict)
        else {}
    )

    assert python.get("min_version") == "3.11"
    assert low_inventory.get("enabled") is True
    assert fields.network == "mainnet"
    assert tx_block.get("mode") == "websocket"
    assert tx_block.get("source") == "coinset"
    assert "key-main-1" in fields.signer_key_registry
    assert fields.signer_key_registry["key-main-1"].get("fingerprint") == 123456789


def test_load_markets_config_smoke() -> None:
    raw = load_yaml(Path("config/markets.yaml"))
    markets = raw.get("markets", [])
    assert len(markets) >= 2
    assert all(
        str(m.get("signer_key_id", "")).strip()
        for m in markets
        if isinstance(m, dict) and m.get("enabled")
    )


def test_load_markets_config_with_optional_overlay(tmp_path: Path) -> None:
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


def test_load_markets_config_rejects_testnet_address_in_base_file(tmp_path: Path) -> None:
    """txch receive_address guard lives in Rust ``load_markets_config_with_overlay``."""
    base_path = tmp_path / "markets.yaml"
    program_path = tmp_path / "program.yaml"
    base_path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: bad_base",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
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
    program_path.write_text(
        "\n".join(
            [
                "app:",
                "  network: mainnet",
                "  home_dir: /tmp/gf-test",
                "runtime:",
                "  loop_interval_seconds: 30",
                "chain_signals:",
                "  tx_block_trigger:",
                '    mode: "websocket"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: true",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 3600",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: false",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
            ]
        ),
        encoding="utf-8",
    )
    code = run_config_validate(
        program_config=program_path,
        markets_config=base_path,
        testnet_markets_config=None,
    )
    assert code != 0


@pytest.mark.skip(
    reason=(
        "Program log_level default/heal behavior is validated in Rust "
        "greenfloor-engine/src/config/program.rs "
        "(parse_program_config_log_level_missing_defaults_to_info, "
        "parse_program_config_log_level_invalid_defaults_to_info)."
    )
)
def test_load_program_config_defaults_log_level_to_info_when_missing(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO\n", "")
    config_path = tmp_path / "program-missing-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    raw = load_yaml(config_path)
    assert raw["app"].get("log_level") is None


@pytest.mark.skip(
    reason=(
        "Program log_level default/heal behavior is validated in Rust "
        "greenfloor-engine/src/config/program.rs."
    )
)
def test_load_program_config_defaults_log_level_to_info_when_invalid(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO", "  log_level: totally-not-a-level")
    config_path = tmp_path / "program-invalid-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    raw = load_yaml(config_path)
    assert raw["app"]["log_level"] == "totally-not-a-level"


def test_config_validate_accepts_example_configs(tmp_path: Path) -> None:
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
