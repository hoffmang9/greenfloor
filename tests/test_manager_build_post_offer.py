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


def test_build_and_post_offer_argv_passes_optional_overrides(tmp_path: Path) -> None:
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
        persist_results=False,
    )
    assert argv[0] == str(binary)
    assert "build-and-post-offer" in argv
    assert "--allow-take" in argv
    assert "--claim-rewards" in argv
    assert "--dry-run" in argv
    assert "--json" in argv
    assert "--no-persist-results" in argv
    assert "--pair" in argv
    assert "A1:txch" in argv


def test_build_and_post_offer_argv_omits_unset_overrides(tmp_path: Path) -> None:
    binary = tmp_path / "greenfloor-engine"
    argv = build_and_post_offer_argv(
        binary=binary,
        program_path=tmp_path / "program.yaml",
        markets_path=tmp_path / "markets.yaml",
        testnet_markets_path=None,
        network="mainnet",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue=None,
        dexie_base_url=None,
        splash_base_url=None,
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
        compact_json=False,
        persist_results=True,
    )
    assert "--venue" not in argv
    assert "--dexie-base-url" not in argv
    assert "--splash-base-url" not in argv


def test_build_and_post_offer_delegates_to_engine(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    program.write_text("app: {}\n", encoding="utf-8")
    markets.write_text("markets: []\n", encoding="utf-8")
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
        publish_venue=None,
        dexie_base_url=None,
        splash_base_url=None,
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 0
    assert captured["market_id"] == "m1"
    assert captured["publish_venue"] is None
    assert captured["dexie_base_url"] is None

    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "dexie"


def test_build_and_post_offer_dry_run_delegates(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    program.write_text("app: {}\n", encoding="utf-8")
    markets.write_text("markets: []\n", encoding="utf-8")
    patch_engine_build_and_post(
        monkeypatch,
        payload=default_build_post_success_payload(
            dry_run=True,
            publish_attempts=0,
            publish_failures=0,
            results=[],
            built_offers_preview=[
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
        repeat=1,
        publish_venue=None,
        dexie_base_url=None,
        splash_base_url=None,
        drop_only=True,
        claim_rewards=False,
        dry_run=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["dry_run"] is True
    assert payload["results"] == []


def test_build_and_post_offer_returns_nonzero_when_publish_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    program.write_text("app: {}\n", encoding="utf-8")
    markets.write_text("markets: []\n", encoding="utf-8")
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
        splash_base_url=None,
        drop_only=True,
        claim_rewards=False,
        dry_run=False,
    )
    assert code == 2


def test_build_and_post_offer_rejects_invalid_repeat(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="repeat must be positive"):
        build_and_post_offer_cli(
            program_path=tmp_path / "program.yaml",
            markets_path=tmp_path / "markets.yaml",
            network="mainnet",
            market_id="m1",
            pair=None,
            size_base_units=1,
            repeat=0,
            publish_venue=None,
            dexie_base_url=None,
            splash_base_url=None,
            drop_only=True,
            claim_rewards=False,
            dry_run=False,
        )
