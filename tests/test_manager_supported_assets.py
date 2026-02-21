from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.manager import _list_supported_assets


def test_list_supported_assets_reads_example_block(tmp_path: Path, capsys) -> None:
    markets = tmp_path / "markets.yaml"
    markets.write_text(
        "\n".join(
            [
                "supported_assets_example:",
                "  - name: Asset A",
                "    base_symbol: A",
                "    asset_id: a1",
                "    legacy_usd_price_per_credit: 1.0",
                "markets: []",
            ]
        ),
        encoding="utf-8",
    )

    code = _list_supported_assets(markets)
    assert code == 0
    out = json.loads(capsys.readouterr().out.strip())
    assert out["count"] == 1
    assert out["assets"][0]["name"] == "Asset A"


def test_list_supported_assets_handles_missing_block(tmp_path: Path, capsys) -> None:
    markets = tmp_path / "markets.yaml"
    markets.write_text("markets: []\n", encoding="utf-8")

    code = _list_supported_assets(markets)
    assert code == 0
    out = json.loads(capsys.readouterr().out.strip())
    assert out["count"] == 0
    assert out["assets"] == []
