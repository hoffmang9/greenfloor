from __future__ import annotations

import json
from pathlib import Path

import pytest

from greenfloor.cli.engine_binary import (
    GreenfloorEngineBinaryError,
    build_and_post_offer_argv,
    resolve_greenfloor_engine_binary,
)
from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.engine_binary_fixtures import (
    default_build_post_success_payload,
    patch_engine_build_and_post,
)
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program_with_signer,
    write_markets,
    write_markets_with_duplicate_pair,
)


def test_resolve_greenfloor_engine_binary_from_env(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    binary = tmp_path / "greenfloor-engine"
    binary.write_text("#!/bin/sh\n", encoding="utf-8")
    binary.chmod(0o755)
    monkeypatch.setenv("GREENFLOOR_ENGINE_BIN", str(binary))
    assert resolve_greenfloor_engine_binary() == binary


def test_resolve_greenfloor_engine_binary_missing_env(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("GREENFLOOR_ENGINE_BIN", raising=False)
    monkeypatch.setattr(
        "greenfloor.cli.engine_binary.shutil.which",
        lambda _name: None,
    )
    monkeypatch.setattr(
        "greenfloor.cli.engine_binary.repo_root",
        lambda: Path("/nonexistent"),
    )
    with pytest.raises(GreenfloorEngineBinaryError, match="binary not found"):
        resolve_greenfloor_engine_binary()


def test_build_and_post_offer_argv_includes_manager_flags(tmp_path: Path) -> None:
    binary = tmp_path / "greenfloor-engine"
    argv = build_and_post_offer_argv(
        binary=binary,
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        testnet_markets_path=tmp_path / "testnet-markets.yaml",
        network="testnet11",
        market_id=None,
        pair="A1:txch",
        size_base_units=10,
        repeat=2,
        publish_venue="dexie",
        dexie_base_url="https://api-testnet.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=False,
        claim_rewards=True,
        dry_run=True,
        compact_json=True,
    )
    assert argv[0] == str(binary)
    assert "build-and-post-offer" in argv
    assert "--allow-take" in argv
    assert "--claim-rewards" in argv
    assert "--dry-run" in argv
    assert "--json" in argv
    assert "--pair" in argv
    assert "A1:txch" in argv


def test_build_and_post_offer_defaults_to_mainnet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    captured: dict = {}
    patch_engine_build_and_post(
        monkeypatch,
        capture=captured,
        payload=default_build_post_success_payload(),
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
    assert captured["dexie_base_url"] == "https://api.dexie.space"
    assert captured["drop_only"] is True
    assert captured["claim_rewards"] is False

    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "dexie"
    assert payload["results"][0]["result"]["id"] == "offer-123"
    assert (
        payload["results"][0]["result"]["offer_view_url"] == "https://dexie.space/offers/offer-123"
    )


def test_build_and_post_offer_dry_run_builds_but_does_not_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    patch_engine_build_and_post(
        monkeypatch,
        payload=default_build_post_success_payload(
            dry_run=True,
            publish_attempts=0,
            publish_failures=0,
            results=[],
            built_offers_preview=[
                {"offer_prefix": "offer1abc", "offer_length": "120"},
                {"offer_prefix": "offer1abc", "offer_length": "120"},
            ],
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=2,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["dry_run"] is True
    assert len(payload["built_offers_preview"]) == 2
    assert payload["results"] == []


def test_build_and_post_offer_resolves_market_by_pair(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    patch_engine_build_and_post(
        monkeypatch,
        payload=default_build_post_success_payload(
            results=[
                {
                    "venue": "dexie",
                    "result": {"success": True, "id": "offer-xyz"},
                }
            ]
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id=None,
        pair="A1:xch",
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
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["market_id"] == "m1"
    assert payload["results"][0]["result"]["id"] == "offer-xyz"


def test_build_and_post_offer_accepts_txch_pair_on_testnet11(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    patch_engine_build_and_post(
        monkeypatch,
        payload=default_build_post_success_payload(
            results=[
                {
                    "venue": "dexie",
                    "result": {
                        "success": True,
                        "id": "offer-txch",
                        "offer_view_url": "https://testnet.dexie.space/offers/offer-txch",
                    },
                }
            ]
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="testnet11",
        market_id=None,
        pair="A1:txch",
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api-testnet.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["result"]["id"] == "offer-txch"


def test_build_and_post_offer_rejects_txch_pair_on_mainnet(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    with pytest.raises(ValueError, match="no enabled market found for pair"):
        build_and_post_offer_cli(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id=None,
            pair="A1:txch",
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )


def test_build_and_post_offer_pair_ambiguous_requires_market_id(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets_with_duplicate_pair(markets)

    with pytest.raises(ValueError, match="ambiguous"):
        build_and_post_offer_cli(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id=None,
            pair="a1:xch",
            size_base_units=10,
            repeat=1,
            publish_venue="dexie",
            dexie_base_url="https://api.dexie.space",
            splash_base_url="http://localhost:4000",
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )


def test_build_and_post_offer_rejects_unknown_market(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    with pytest.raises(ValueError, match="market_id not found"):
        build_and_post_offer_cli(
            program_path=program,
            markets_path=markets,
            network="mainnet",
            market_id="missing",
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


def test_build_and_post_offer_posts_to_splash_when_selected(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    patch_engine_build_and_post(
        monkeypatch,
        payload=default_build_post_success_payload(
            publish_venue="splash",
            results=[{"venue": "splash", "result": {"success": True, "id": "splash-1"}}],
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="splash",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "splash"


def test_build_and_post_offer_returns_nonzero_when_publish_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    patch_engine_build_and_post(
        monkeypatch,
        exit_code=2,
        payload=default_build_post_success_payload(
            publish_failures=1,
            results=[
                {
                    "venue": "dexie",
                    "result": {"success": False, "error": "dexie_http_error:500"},
                }
            ],
        ),
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
