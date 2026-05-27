"""Shared fixtures for offer runtime and publish tests."""

from __future__ import annotations

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
                "cloud_wallet:",
                '  base_url: ""',
                '  user_key_id: ""',
                '  private_key_pem_path: ""',
                '  vault_id: ""',
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


def write_markets(path: Path) -> None:
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
                '    signer_key_id: "k1"',
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
                '    signer_key_id: "k1"',
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


def write_program_with_cloud_wallet(
    path: Path,
    *,
    provider: str = "dexie",
    with_kms: bool = False,
    home_dir: str | None = None,
) -> None:
    """Write a program.yaml with valid Cloud Wallet credentials populated."""
    write_program(path, provider=provider, home_dir=home_dir)
    text = path.read_text(encoding="utf-8")
    text = text.replace('  base_url: ""', '  base_url: "https://wallet.example.com"')
    text = text.replace('  user_key_id: ""', '  user_key_id: "key-1"')
    text = text.replace('  private_key_pem_path: ""', '  private_key_pem_path: "/tmp/key.pem"')
    text = text.replace('  vault_id: ""', '  vault_id: "wallet-1"')
    if with_kms:
        text = text.replace(
            '  kms_key_id: ""', '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"'
        )
        text = text.replace('  kms_region: ""', '  kms_region: "us-west-2"')
        text = text.replace('  kms_public_key_hex: ""', '  kms_public_key_hex: "02abc123"')
        if "signer:" not in text:
            text = text.replace(
                "coin_ops:",
                "\n".join(
                    [
                        "signer:",
                        '  kms_key_id: "arn:aws:kms:us-west-2:123:key/demo"',
                        '  kms_region: "us-west-2"',
                        '  kms_public_key_hex: "02abc123"',
                        "vault:",
                        '  launcher_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"',
                        "  custody_threshold: 1",
                        "  recovery_threshold: 1",
                        "  recovery_clawback_timelock: 3600",
                        "  custody_keys:",
                        '    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"',
                        "      curve: SECP256R1",
                        "  recovery_keys:",
                        '    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"',
                        "      curve: BLS12_381",
                        "",
                        "coin_ops:",
                    ]
                ),
            )
    path.write_text(text, encoding="utf-8")


def write_markets_with_duplicate_pair(path: Path) -> None:
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
                '    signer_key_id: "k1"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 10",
                "    pricing:",
                "      min_price_quote_per_base: 0.0031",
                "      max_price_quote_per_base: 0.0038",
                "  - id: m2",
                "    enabled: true",
                '    base_asset: "a1"',
                '    base_symbol: "A1"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "k1"',
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


def load_program_and_market(program_path: Path, markets_path: Path):
    from greenfloor.config.io import load_markets_config, load_program_config

    prog = load_program_config(program_path)
    mkt = load_markets_config(markets_path).markets[0]
    return prog, mkt
