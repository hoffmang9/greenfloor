from __future__ import annotations

import importlib.util
import sys
from pathlib import Path
from typing import Any


def _load_script_module() -> Any:
    script_path = (
        Path(__file__).resolve().parents[1] / "scripts" / "combine_market_cat_dust_coinset.py"
    )
    spec = importlib.util.spec_from_file_location("combine_market_cat_dust_coinset", script_path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_dust_coin_ids_from_list_payload_filters_spent_and_threshold() -> None:
    mod = _load_script_module()
    cat = "a" * 64
    payload = {
        "coins": [
            {"coin_id": cat, "type": "CAT", "amount": 500, "spent_block_index": 0},
            {"coin_id": "b" * 64, "type": "CAT", "amount": 1000, "spent_block_index": 0},
            {"coin_id": "c" * 64, "type": "CAT", "amount": 100, "spent_block_index": 1},
            {"coin_id": "d" * 64, "type": "XCH", "amount": 1, "spent_block_index": 0},
        ]
    }
    got = mod._dust_coin_ids_from_list_payload(payload, dust_threshold_mojos=1000)
    assert got == [cat]


def test_chunk_coin_ids_minimum_batch_two() -> None:
    mod = _load_script_module()
    ids = [f"{i:064x}" for i in range(5)]
    assert mod._chunk_coin_ids(ids, 2) == [ids[0:2], ids[2:4], ids[4:5]]
    assert mod._chunk_coin_ids(ids, 10) == [ids]


def test_build_enabled_cat_jobs_resolves_symbol_and_merges(tmp_path: Path) -> None:
    mod = _load_script_module()
    cat_hex = "f" * 64
    markets = tmp_path / "markets.yaml"
    markets.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: hex_m",
                "    enabled: true",
                f'    base_asset: "{cat_hex}"',
                '    base_symbol: "HEX"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-a"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
                "  - id: sym_m",
                "    enabled: true",
                '    base_asset: "ZZT"',
                '    base_symbol: "ZZT"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-a"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
                "  - id: off_m",
                "    enabled: false",
                f'    base_asset: "{cat_hex}"',
                '    base_symbol: "HEX2"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-a"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
            ]
        ),
        encoding="utf-8",
    )
    zzt = "e" * 64
    cats = tmp_path / "cats.yaml"
    cats.write_text(
        "\n".join(
            [
                "cats:",
                "  - name: z",
                '    base_symbol: "ZZT"',
                f'    asset_id: "{zzt}"',
            ]
        ),
        encoding="utf-8",
    )
    jobs = mod._build_enabled_cat_jobs(
        markets_config_path=markets,
        testnet_markets_path=None,
        cats_path=cats,
        only_cat_asset_id=None,
    )
    by_cat = {j.cat_asset_id: j for j in jobs}
    assert set(by_cat) == {cat_hex, zzt}
    assert by_cat[cat_hex].signer_key_id == "key-a"
    assert set(by_cat[zzt].market_ids) == {"sym_m"}


def test_build_enabled_cat_jobs_conflict_receive_address(tmp_path: Path) -> None:
    mod = _load_script_module()
    cat_hex = "f" * 64
    markets = tmp_path / "markets.yaml"
    markets.write_text(
        "\n".join(
            [
                "markets:",
                "  - id: m1",
                "    enabled: true",
                f'    base_asset: "{cat_hex}"',
                '    base_symbol: "HEX"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-a"',
                '    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
                "  - id: m2",
                "    enabled: true",
                f'    base_asset: "{cat_hex}"',
                '    base_symbol: "HEX"',
                '    quote_asset: "xch"',
                '    quote_asset_type: "unstable"',
                '    signer_key_id: "key-a"',
                '    receive_address: "xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w"',
                '    mode: "sell_only"',
                "    inventory:",
                "      low_watermark_base_units: 100",
            ]
        ),
        encoding="utf-8",
    )
    cats = tmp_path / "cats.yaml"
    cats.write_text("cats: []\n", encoding="utf-8")
    try:
        mod._build_enabled_cat_jobs(
            markets_config_path=markets,
            testnet_markets_path=None,
            cats_path=cats,
            only_cat_asset_id=None,
        )
    except ValueError as exc:
        assert "Conflicting receive_address" in str(exc)
    else:
        raise AssertionError("expected ValueError")
