"""Script adapter smoke tests for signer YAML sections (policy is Rust-only)."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import ScriptProgramFields, load_yaml


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
                f"    - public_key_hex: {'0202' * 32}",
                "      curve: SECP256R1",
                "  recovery_keys:",
                '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                "      curve: BLS12_381",
            ]
        ),
        encoding="utf-8",
    )


def test_script_program_fields_reads_signer_sections(tmp_path: Path) -> None:
    program_path = tmp_path / "program.yaml"
    launcher_id = "aa" * 32
    _write_minimal_program(
        program_path,
        kms_key_id="arn:aws:kms:us-west-2:1:key/x",
        launcher_id=launcher_id,
    )

    loaded = load_yaml(program_path)
    fields = ScriptProgramFields.from_raw(loaded)
    assert fields.signer_kms_key_id == "arn:aws:kms:us-west-2:1:key/x"
    assert loaded["vault"]["launcher_id"] == launcher_id
