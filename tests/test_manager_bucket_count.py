import shutil
from pathlib import Path

from greenfloor.cli.manager import _set_bucket_count
from greenfloor.config.io import load_yaml


def test_set_bucket_count_updates_inventory(tmp_path: Path) -> None:
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/markets.yaml", markets)

    code = _set_bucket_count(
        markets_path=markets,
        market_id="carbon_2022_xch_sell",
        size_base_units=10,
        count=7,
        reload=False,
        state_dir=tmp_path / "state",
    )
    assert code == 0
    data = load_yaml(markets)
    market = next(m for m in data["markets"] if m["id"] == "carbon_2022_xch_sell")
    assert int(market["inventory"]["bucket_counts"]["10"]) == 7


def test_set_bucket_count_writes_reload_marker(tmp_path: Path) -> None:
    markets = tmp_path / "markets.yaml"
    shutil.copyfile("config/markets.yaml", markets)
    state_dir = tmp_path / "state"

    code = _set_bucket_count(
        markets_path=markets,
        market_id="carbon_2022_xch_sell",
        size_base_units=1,
        count=5,
        reload=True,
        state_dir=state_dir,
    )
    assert code == 0
    assert (state_dir / "reload_request.json").exists()
