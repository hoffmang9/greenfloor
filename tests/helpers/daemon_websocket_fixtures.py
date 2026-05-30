"""Shared YAML/config helpers for daemon run_once/run_loop tests."""

from __future__ import annotations

from pathlib import Path


def write_program(
    path: Path,
    home_dir: Path,
    *,
    parallel_markets: bool = False,
    dexie_api_base: str = "https://api.dexie.space",
) -> None:
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{str(home_dir)}"',
                "  log_level: INFO",
                "runtime:",
                "  loop_interval_seconds: 30",
                "  dry_run: false",
                f"  parallel_markets: {'true' if parallel_markets else 'false'}",
                "chain_signals:",
                "  tx_block_trigger:",
                '    mode: "websocket"',
                '    websocket_url: "wss://coinset.org/ws"',
                "    websocket_reconnect_interval_seconds: 1",
                "    fallback_poll_interval_seconds: 1",
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: false",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: false",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                f'    api_base: "{dexie_api_base}"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                '    provider: "dexie"',
                "coin_ops:",
                "  max_operations_per_run: 0",
                "  max_daily_fee_budget_mojos: 0",
                "  split_fee_mojos: 0",
                "  combine_fee_mojos: 0",
                "keys:",
                "  registry:",
                '    - key_id: "key-main-1"',
                "      fingerprint: 123456789",
                '      network: "mainnet"',
                '      keyring_yaml_path: "~/.chia_keys/keyring.yaml"',
            ]
        ),
        encoding="utf-8",
    )


def write_program_without_log_level(path: Path, home_dir: Path) -> None:
    write_program(path, home_dir)
    text = path.read_text(encoding="utf-8")
    path.write_text(text.replace("  log_level: INFO\n", ""), encoding="utf-8")


def write_markets(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "asset1"',
                '    base_symbol: "AS1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "      bucket_counts:",
                "        1: 0",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 1",
                "          target_count: 1",
                "          split_buffer_count: 0",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )


def write_markets_two(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "asset1"',
                '    base_symbol: "AS1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "      bucket_counts:",
                "        1: 0",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 1",
                "          target_count: 1",
                "          split_buffer_count: 0",
                "          combine_when_excess_factor: 2.0",
                "  - id: m2",
                "    enabled: true",
                '    base_asset: "asset2"',
                '    base_symbol: "AS2"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "      bucket_counts:",
                "        1: 0",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 1",
                "          target_count: 1",
                "          split_buffer_count: 0",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )
