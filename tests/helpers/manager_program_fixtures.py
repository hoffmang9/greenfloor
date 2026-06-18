"""Program and markets YAML fixtures for native manager CLI tests."""

from __future__ import annotations

import shutil
from pathlib import Path


def write_program(path: Path, *, provider: str = "dexie", home_dir: str | None = None) -> None:
    home_yaml = "~/.greenfloor" if home_dir is None else str(home_dir).replace("\\", "/")
    path.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{home_yaml}"',
                "runtime:",
                "  loop_interval_seconds: 30",
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
                "dev:",
                "  python:",
                '    min_version: "3.11"',
                "notifications:",
                "  low_inventory_alerts:",
                "    enabled: true",
                '    threshold_mode: "absolute_base_units"',
                "    default_threshold_base_units: 0",
                "    dedup_cooldown_seconds: 60",
                "    clear_hysteresis_percent: 10",
                "  providers:",
                "    - type: pushover",
                "      enabled: true",
                '      user_key_env: "PUSHOVER_USER_KEY"',
                '      app_token_env: "PUSHOVER_APP_TOKEN"',
                '      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"',
                "venues:",
                "  dexie:",
                '    api_base: "https://api.dexie.space"',
                "  splash:",
                '    api_base: "http://localhost:4000"',
                "  offer_publish:",
                f'    provider: "{provider}"',
            ]
        ),
        encoding="utf-8",
    )


def write_manager_program(path: Path, *, tmp_path: Path, provider: str = "dexie") -> None:
    write_program(path, provider=provider, home_dir=str(tmp_path))


def write_manager_program_with_signer(path: Path, *, tmp_path: Path) -> None:
    """Copy repo program.yaml (signer + vault) with home_dir under tmp_path."""
    shutil.copyfile("config/program.yaml", path)
    text = path.read_text(encoding="utf-8")
    home_yaml = str(tmp_path).replace("\\", "/")
    if 'home_dir: "~/.greenfloor"' in text:
        text = text.replace('home_dir: "~/.greenfloor"', f'home_dir: "{home_yaml}"')
    else:
        text = text.replace("home_dir:", f'home_dir: "{home_yaml}"', 1)
    if 'kms_key_id: ""' in text:
        text = text.replace(
            'kms_key_id: ""',
            'kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"',
            1,
        )
    if 'kms_region: ""' in text:
        text = text.replace('kms_region: ""', 'kms_region: "us-west-2"', 1)
    if 'kms_public_key_hex: ""' in text:
        text = text.replace(
            'kms_public_key_hex: ""',
            'kms_public_key_hex: "02abc123"',
            1,
        )
    path.write_text(text, encoding="utf-8")


def write_markets_with_ladder(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 10",
                "          target_count: 3",
                "          split_buffer_count: 1",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )


CAT_ASSET_HEX = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"


def write_markets_cat_for_coin_ops(path: Path) -> None:
    path.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                f'    base_asset: "{CAT_ASSET_HEX}"',
                '    base_symbol: "TCAT"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-main-1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "    ladders:",
                "      sell:",
                "        - size_base_units: 10",
                "          target_count: 2",
                "          split_buffer_count: 1",
                "          combine_when_excess_factor: 2.0",
            ]
        ),
        encoding="utf-8",
    )
