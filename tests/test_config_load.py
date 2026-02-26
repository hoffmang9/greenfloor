from pathlib import Path

import pytest

from greenfloor.config.io import (
    load_markets_config,
    load_markets_config_with_optional_overlay,
    load_program_config,
)


def test_load_program_config() -> None:
    cfg = load_program_config(Path("config/program.yaml"))
    assert cfg.python_min_version == "3.11"
    assert cfg.low_inventory_enabled is True
    assert cfg.app_log_level == "INFO"
    assert cfg.tx_block_trigger_mode == "websocket"
    assert cfg.tx_block_websocket_url.startswith("wss://")
    assert "key-main-1" in cfg.signer_key_registry
    assert cfg.signer_key_registry["key-main-1"].fingerprint == 123456789


def test_load_markets_config() -> None:
    cfg = load_markets_config(Path("config/markets.yaml"))
    assert len(cfg.markets) >= 2
    assert all(m.signer_key_id for m in cfg.markets if m.enabled)


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
    cfg = load_markets_config_with_optional_overlay(path=base_path, overlay_path=overlay_path)
    assert len(cfg.markets) == 2
    assert {m.market_id for m in cfg.markets} == {"base_m1", "testnet_m1"}


def test_load_markets_config_rejects_testnet_address_in_base_file(tmp_path: Path) -> None:
    base_path = tmp_path / "markets.yaml"
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
    with pytest.raises(ValueError, match="testnet receive_address entries found"):
        load_markets_config_with_optional_overlay(path=base_path, overlay_path=None)


def test_load_program_config_defaults_log_level_to_info_when_missing(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO\n", "")
    config_path = tmp_path / "program-missing-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    cfg = load_program_config(config_path)
    assert cfg.app_log_level == "INFO"
    rewritten = config_path.read_text(encoding="utf-8")
    assert "log_level: INFO" in rewritten


def test_load_program_config_defaults_log_level_to_info_when_invalid(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO", "  log_level: totally-not-a-level")
    config_path = tmp_path / "program-invalid-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    cfg = load_program_config(config_path)
    assert cfg.app_log_level == "INFO"
