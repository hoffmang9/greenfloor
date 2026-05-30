from __future__ import annotations

from pathlib import Path

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.engine_binary_fixtures import patch_engine_build_and_post
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_signer,
    write_markets,
)


def test_build_and_post_offer_cli_delegates_to_engine_in_process(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    captured: dict = {}
    patch_engine_build_and_post(monkeypatch, capture=captured)

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=10,
        repeat=1,
        publish_venue=None,
        dexie_base_url=None,
        splash_base_url=None,
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    assert captured["program_path"] == program.expanduser().resolve()
    assert captured["markets_path"] == markets.expanduser().resolve()
    assert captured["market_id"] == "m1"
