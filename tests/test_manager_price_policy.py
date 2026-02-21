from pathlib import Path

from greenfloor.cli.manager import _set_price_policy
from greenfloor.config.io import load_yaml


def test_set_price_policy_updates_yaml_and_history(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    db = tmp_path / "db.sqlite"

    program.write_text(
        "\n".join(
            [
                "app:",
                '  network: "mainnet"',
                f'  home_dir: "{tmp_path.as_posix()}"',
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
    markets.write_text(
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
                "    pricing:",
                "      slippage_bps: 100",
                "    inventory:",
                "      low_watermark_base_units: 10",
            ]
        ),
        encoding="utf-8",
    )

    code = _set_price_policy(
        program_path=program,
        markets_path=markets,
        market_id="m1",
        policy_items=["slippage_bps=75", "min_price_quote_per_base=0.12"],
        state_db=str(db),
    )
    assert code == 0
    data = load_yaml(markets)
    pricing = data["markets"][0]["pricing"]
    assert pricing["slippage_bps"] == 75
    assert pricing["min_price_quote_per_base"] == 0.12
