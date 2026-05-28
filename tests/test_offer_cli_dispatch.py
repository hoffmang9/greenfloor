from __future__ import annotations

from pathlib import Path

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.fake_adapters import FakeDexie
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_manager_program_with_signer,
    write_markets,
)


def testbuild_and_post_offer_cli_uses_local_path_when_signer_not_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    signer_dispatched = [False]
    local_builder_calls = [0]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_signer",
        _fake_signer,
    )
    monkeypatch.setattr("greenfloor.cli.offer_build_post.build_offer", lambda payload: "offer1abc")

    class _FakeDexie(FakeDexie):
        offer_id = "local-no-signer"

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None = None):
            _ = offer, drop_only, claim_rewards
            local_builder_calls[0] += 1
            return {"success": True, "id": self.offer_id}

    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie", lambda _offer: None
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
    assert signer_dispatched[0] is False
    assert local_builder_calls[0] == 1


def testbuild_and_post_offer_cli_uses_signer_path_for_kms_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    signer_dispatched = [False]
    local_builder_calls = [0]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_signer",
        _fake_signer,
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
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
    assert local_builder_calls[0] == 0


def testbuild_and_post_offer_cli_uses_signer_path_for_kms_configured_large_size(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    signer_dispatched = [False]
    local_builder_calls = [0]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_signer",
        _fake_signer,
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=100,
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
    assert local_builder_calls[0] == 0
