from __future__ import annotations

from pathlib import Path

import pytest

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_manager_program_with_signer,
    write_markets,
)


def test_build_and_post_offer_cli_requires_signer_config(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    with pytest.raises(
        ValueError,
        match="offer execution requires signer.kms_key_id and vault.launcher_id",
    ):
        build_and_post_offer_cli(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="m1",
            pair=None,
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )


def test_build_and_post_offer_cli_uses_signer_path(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    signer_dispatched = [False]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_signer",
        _fake_signer,
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    assert signer_dispatched[0] is True
