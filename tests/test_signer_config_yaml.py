"""Signer config smoke tests for scripts; offer-path policy is Rust-only."""

from __future__ import annotations

from pathlib import Path

import yaml

from greenfloor.config.io import ScriptProgramFields, load_yaml
from tests.helpers.manager_cli import parse_json_output, run_manager

# ``require_signer_offer_path``, signer.yaml materialization, and runtime cache
# invalidation are covered in Rust (``greenfloor-engine/src/config/program.rs``,
# ``manager_cli/tests.rs::manager_config_and_market_resolution``). These pytest
# cases exercise YAML loading via ``greenfloor.config.io`` and doctor warnings.


def _write_minimal_program(
    path: Path,
    *,
    kms_key_id: str,
    launcher_id: str,
) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                "  network: testnet11",
                "  home_dir: /tmp/gf-test",
                "runtime:",
                "  loop_interval_seconds: 15",
                "chain_signals:",
                "  tx_block_trigger:",
                '    mode: "websocket"',
                '    websocket_url: "wss://testnet11.api.coinset.org/ws"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: false",
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
                "signer:",
                f'  kms_key_id: "{kms_key_id}"',
                '  kms_region: "us-west-2"',
                "vault:",
                f'  launcher_id: "{launcher_id}"',
                "  custody_threshold: 1",
                "  recovery_threshold: 1",
                "  recovery_clawback_timelock: 3600",
                "  custody_keys:",
                f'    - public_key_hex: {"0202" * 32}',
                "      curve: SECP256R1",
                "  recovery_keys:",
                '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                "      curve: BLS12_381",
            ]
        ),
        encoding="utf-8",
    )


def _write_minimal_markets(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "txch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
            ]
        ),
        encoding="utf-8",
    )


def test_signer_yaml_sections_load_via_config_io(tmp_path: Path) -> None:
    program_path = tmp_path / "program.yaml"
    launcher_id = "aa" * 32
    _write_minimal_program(
        program_path,
        kms_key_id="arn:aws:kms:us-west-2:1:key/x",
        launcher_id=launcher_id,
    )

    loaded = load_yaml(program_path)
    assert ScriptProgramFields.from_raw(loaded).signer_kms_key_id == "arn:aws:kms:us-west-2:1:key/x"
    assert loaded["vault"]["launcher_id"] == launcher_id


def test_doctor_warns_when_signer_offer_path_not_configured(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_minimal_program(program, kms_key_id="", launcher_id="")
    _write_minimal_markets(markets)

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "--state-db",
            str(tmp_path / "state.sqlite"),
            "doctor",
        ]
    )
    payload = parse_json_output(stdout)
    assert code == 0
    assert "signer_not_configured:kms_key_id_or_vault_launcher_id" in payload.get("warnings", [])


def test_doctor_ok_when_signer_kms_and_vault_present(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    _write_minimal_program(program, kms_key_id="arn:x", launcher_id="aa" * 32)
    _write_minimal_markets(markets)

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "--markets-config",
            str(markets),
            "--state-db",
            str(tmp_path / "state.sqlite"),
            "doctor",
        ]
    )
    payload = parse_json_output(stdout)
    assert code == 0
    assert "signer_not_configured:kms_key_id_or_vault_launcher_id" not in payload.get("warnings", [])
