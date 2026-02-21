import shutil
from pathlib import Path

from greenfloor.cli.manager import _set_ladder_entry
from greenfloor.config.io import load_yaml


def test_set_ladder_entry_updates_existing(tmp_path: Path) -> None:
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/markets.yaml", markets)

    code = _set_ladder_entry(
        markets_path=markets,
        market_id="carbon_2022_xch_sell",
        side="sell",
        size_base_units=10,
        target_count=9,
        split_buffer_count=2,
        combine_when_excess_factor=2.5,
        reload=False,
        state_dir=tmp_path / "state",
    )
    assert code == 0
    data = load_yaml(markets)
    market = next(m for m in data["markets"] if m["id"] == "carbon_2022_xch_sell")
    entry = next(e for e in market["ladders"]["sell"] if int(e["size_base_units"]) == 10)
    assert int(entry["target_count"]) == 9
    assert int(entry["split_buffer_count"]) == 2
    assert float(entry["combine_when_excess_factor"]) == 2.5


def test_set_ladder_entry_adds_new_size(tmp_path: Path) -> None:
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/markets.yaml", markets)

    code = _set_ladder_entry(
        markets_path=markets,
        market_id="carbon_2022_xch_sell",
        side="sell",
        size_base_units=250,
        target_count=1,
        split_buffer_count=0,
        combine_when_excess_factor=2.0,
        reload=False,
        state_dir=tmp_path / "state",
    )
    assert code == 0
    data = load_yaml(markets)
    market = next(m for m in data["markets"] if m["id"] == "carbon_2022_xch_sell")
    entry = next(e for e in market["ladders"]["sell"] if int(e["size_base_units"]) == 250)
    assert int(entry["target_count"]) == 1
