from __future__ import annotations

from pathlib import Path

import yaml

from greenfloor.cli.manager import _bootstrap_home


def _write_templates(root: Path) -> tuple[Path, Path, Path, Path]:
    program_template = root / "program.template.yaml"
    markets_template = root / "markets.template.yaml"
    cats_template = root / "cats.template.yaml"
    testnet_markets_template = root / "testnet-markets.template.yaml"
    program_template.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                '  home_dir: "~/.greenfloor"',
                "runtime:",
                "  loop_interval_seconds: 30",
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
                "chain_signals:",
                "  tx_block_trigger:",
                "    webhook_enabled: true",
                '    webhook_listen_addr: "127.0.0.1:8787"',
            ]
        ),
        encoding="utf-8",
    )
    markets_template.write_text(
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
                "      low_watermark_base_units: 100",
            ]
        ),
        encoding="utf-8",
    )
    cats_template.write_text(
        "\n".join(
            [
                "cats:",
                "  - name: Token One",
                '    base_symbol: "TOK1"',
                '    asset_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"',
                "    target_usd_per_unit: null",
                "    dexie:",
                "      ticker_id: null",
                "      pool_id: null",
                "      last_price_xch: null",
            ]
        ),
        encoding="utf-8",
    )
    testnet_markets_template.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m-testnet",
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
    return program_template, markets_template, cats_template, testnet_markets_template


def test_bootstrap_home_creates_layout_and_seed_configs(tmp_path: Path) -> None:
    home_dir = tmp_path / ".greenfloor"
    program_template, markets_template, cats_template, testnet_markets_template = _write_templates(
        tmp_path
    )

    code = _bootstrap_home(
        home_dir=home_dir,
        program_template=program_template,
        markets_template=markets_template,
        cats_template=cats_template,
        testnet_markets_template=testnet_markets_template,
        seed_testnet_markets=False,
        force=False,
    )

    assert code == 0
    assert (home_dir / "config").is_dir()
    assert (home_dir / "db").is_dir()
    assert (home_dir / "state").is_dir()
    assert (home_dir / "logs").is_dir()
    assert (home_dir / "db" / "greenfloor.sqlite").is_file()
    assert (home_dir / "config" / "program.yaml").is_file()
    assert (home_dir / "config" / "markets.yaml").is_file()
    assert (home_dir / "config" / "cats.yaml").is_file()

    seeded_program = yaml.safe_load(
        (home_dir / "config" / "program.yaml").read_text(encoding="utf-8")
    )
    assert seeded_program["app"]["home_dir"] == str(home_dir.resolve())


def test_bootstrap_home_without_force_keeps_existing_seeded_config(tmp_path: Path) -> None:
    home_dir = tmp_path / ".greenfloor"
    program_template, markets_template, cats_template, testnet_markets_template = _write_templates(
        tmp_path
    )
    (home_dir / "config").mkdir(parents=True, exist_ok=True)
    (home_dir / "config" / "program.yaml").write_text(
        'app:\n  home_dir: "custom-home"\n',
        encoding="utf-8",
    )
    (home_dir / "config" / "markets.yaml").write_text(
        "markets: []\n",
        encoding="utf-8",
    )
    (home_dir / "config" / "cats.yaml").write_text(
        "cats: []\n",
        encoding="utf-8",
    )

    code = _bootstrap_home(
        home_dir=home_dir,
        program_template=program_template,
        markets_template=markets_template,
        cats_template=cats_template,
        testnet_markets_template=testnet_markets_template,
        seed_testnet_markets=False,
        force=False,
    )

    assert code == 0
    assert (home_dir / "config" / "program.yaml").read_text(encoding="utf-8") == (
        'app:\n  home_dir: "custom-home"\n'
    )
    assert (home_dir / "config" / "markets.yaml").read_text(encoding="utf-8") == "markets: []\n"
    assert (home_dir / "config" / "cats.yaml").read_text(encoding="utf-8") == "cats: []\n"


def test_bootstrap_home_can_seed_optional_testnet_markets(tmp_path: Path) -> None:
    home_dir = tmp_path / ".greenfloor"
    program_template, markets_template, cats_template, testnet_markets_template = _write_templates(
        tmp_path
    )

    code = _bootstrap_home(
        home_dir=home_dir,
        program_template=program_template,
        markets_template=markets_template,
        cats_template=cats_template,
        testnet_markets_template=testnet_markets_template,
        seed_testnet_markets=True,
        force=False,
    )

    assert code == 0
    assert (home_dir / "config" / "testnet-markets.yaml").is_file()
