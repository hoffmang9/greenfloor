from __future__ import annotations

import json
from pathlib import Path

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.fake_adapters import FakeDexie
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_manager_program_with_cloud_wallet,
    write_markets,
)


def testbuild_and_post_offer_cli_dispatches_to_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
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
    assert dispatched[0] is True
    assert captured_dry_run == [False]


def testbuild_and_post_offer_cli_dry_run_uses_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        print(json.dumps({"dry_run": True, "results": [], "built_offers_preview": []}))
        return 0, {"dry_run": True, "results": [], "built_offers_preview": []}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
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
        dry_run=True,
    )
    assert code == 0
    assert dispatched[0] is True
    assert captured_dry_run == [True]
    payload = json.loads(capsys.readouterr().out.strip().splitlines()[-1])
    assert payload["dry_run"] is True


def testbuild_and_post_offer_cli_uses_local_path_for_large_size_when_cloud_wallet_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path)
    write_markets(markets)

    cloud_dispatched = [False]
    local_builder_calls = [0]

    def _fake_cloud_wallet(**kwargs):
        _ = kwargs
        cloud_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer_text", lambda payload: "offer1abc"
    )

    class _FakeDexie(FakeDexie):
        offer_id = "local-100-id"

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None = None):
            _ = offer, drop_only, claim_rewards
            local_builder_calls[0] += 1
            return {"success": True, "id": self.offer_id}

    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.runtime.offer_orchestration.verify_offer_text_for_dexie", lambda _offer: None
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
    assert cloud_dispatched[0] is False
    assert local_builder_calls[0] == 1


def testbuild_and_post_offer_cli_uses_local_path_when_cloud_wallet_not_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    cloud_dispatched = [False]
    local_builder_calls = [0]

    def _fake_cloud_wallet(**kwargs):
        _ = kwargs
        cloud_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.runtime.offer_post_request.build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer_text", lambda payload: "offer1abc"
    )

    class _FakeDexie(FakeDexie):
        offer_id = "local-no-cw"

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None = None):
            _ = offer, drop_only, claim_rewards
            local_builder_calls[0] += 1
            return {"success": True, "id": self.offer_id}

    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.runtime.offer_orchestration.verify_offer_text_for_dexie", lambda _offer: None
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
    assert cloud_dispatched[0] is False
    assert local_builder_calls[0] == 1


def testbuild_and_post_offer_cli_uses_signer_path_for_kms_configured(
    monkeypatch, tmp_path: Path
) -> None:
    """KMS-configured runs must use the local Rust signer path for all sizes."""
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, with_kms=True)
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
        "greenfloor.cli.offer_build_post.build_offer_text",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.offer_execution_backend",
        lambda _program, **kwargs: "signer",
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
    """KMS-configured runs use signer path even for size >= 100."""
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program, tmp_path=tmp_path, with_kms=True)
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
        "greenfloor.cli.offer_build_post.build_offer_text",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
    )
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.offer_execution_backend",
        lambda _program, **kwargs: "signer",
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
