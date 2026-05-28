from __future__ import annotations

import json
import sys
from pathlib import Path

import yaml

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.fake_adapters import FakeDexie
from tests.helpers.offer_runtime_fixtures import (
    write_manager_program,
    write_markets,
    write_markets_with_duplicate_pair,
)


def test_build_and_post_offer_defaults_to_mainnet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    captured: dict = {}

    class _FakeDexie(FakeDexie):
        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None = None):
            captured["base_url"] = self.base_url
            captured["offer"] = offer
            captured["drop_only"] = drop_only
            captured["claim_rewards"] = claim_rewards
            return {"success": True, "id": "offer-123"}

        def get_offer(self, offer_id: str) -> dict:
            return super().get_offer(offer_id)

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1abc",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    assert captured["base_url"] == "https://api.dexie.space"
    assert captured["offer"] == "offer1abc"
    assert captured["drop_only"] is True
    assert captured["claim_rewards"] is False

    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["venue"] == "dexie"
    assert payload["results"][0]["result"]["id"] == "offer-123"
    assert (
        payload["results"][0]["result"]["offer_view_url"] == "https://dexie.space/offers/offer-123"
    )


def test_build_and_post_offer_local_path_persists_sqlite_audit_record(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1abc",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    _ = capsys.readouterr()

    db_path = (tmp_path / "db" / "greenfloor.sqlite").resolve()
    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(
            event_types=["strategy_offer_execution"],
            market_id="m1",
            limit=1,
        )
    finally:
        store.close()
    assert len(events) == 1
    items = list((events[0].get("payload") or {}).get("items") or [])
    assert len(items) == 1
    assert items[0]["offer_id"] == "offer-123"


def test_build_and_post_offer_uses_market_configured_expiry_override(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    raw = yaml.safe_load(markets.read_text(encoding="utf-8"))
    pricing = dict(raw["markets"][0].get("pricing") or {})
    pricing["strategy_offer_expiry_minutes"] = 12
    raw["markets"][0]["pricing"] = pricing
    markets.write_text(yaml.safe_dump(raw, sort_keys=False), encoding="utf-8")

    captured_payload: dict[str, object] = {}

    class _FakeDexie(FakeDexie):
        offer_id = "offer-expiry-1"

    def _fake_build(payload: dict) -> str:
        captured_payload.update(payload)
        return "offer1expiryoverride"

    monkeypatch.setattr("greenfloor.cli.offer_build_post.build_offer", _fake_build)
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
    assert captured_payload["expiry_unit"] == "minutes"
    assert captured_payload["expiry_value"] == 12
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0
    assert payload["results"][0]["result"]["id"] == "offer-expiry-1"


def test_build_and_post_offer_dry_run_builds_but_does_not_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FailDexie:
        def __init__(self, _base_url: str) -> None:
            raise AssertionError("DexieAdapter should not be constructed in dry_run")

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1dryrun",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FailDexie)

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


def test_build_and_post_offer_dry_run_can_capture_full_offer_text(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    capture_dir = tmp_path / "offer-capture"

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1captureme",
    )
    monkeypatch.setenv("GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", str(capture_dir))
    try:
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
            dry_run=True,
        )
    finally:
        monkeypatch.delenv("GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", raising=False)

    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    capture_path = Path(payload["built_offers_preview"][0]["offer_capture_path"])
    assert capture_path.exists()
    assert capture_path.read_text(encoding="utf-8") == "offer1captureme"


def test_build_and_post_offer_resolves_market_by_pair(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie(FakeDexie):
        offer_id = "offer-xyz"

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    assert payload["results"][0]["venue"] == "dexie"
    assert payload["results"][0]["result"]["id"] == "offer-xyz"


def test_build_and_post_offer_accepts_txch_pair_on_testnet11(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie(FakeDexie):
        offer_id = "offer-txch"

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    assert payload["market_id"] == "m1"
    assert payload["results"][0]["result"]["id"] == "offer-txch"
    assert payload["results"][0]["result"]["offer_view_url"] == (
        "https://testnet.dexie.space/offers/offer-txch"
    )


def test_build_and_post_offer_rejects_txch_pair_on_mainnet(tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    try:
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
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "no enabled market found for pair" in str(exc)


def test_build_and_post_offer_pair_ambiguous_requires_market_id(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets_with_duplicate_pair(markets)
    try:
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
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "ambiguous" in str(exc)


def test_build_and_post_offer_rejects_unknown_market(monkeypatch, tmp_path: Path) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    try:
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
        raise AssertionError("expected ValueError")
    except ValueError as exc:
        assert "market_id not found" in str(exc)


def test_build_and_post_offer_posts_to_splash_when_selected(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeSplash:
        def __init__(self, base_url: str) -> None:
            self.base_url = base_url

        def post_offer(self, offer: str):
            _ = offer
            return {"success": True, "id": "splash-1"}

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1pair",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.SplashAdapter", _FakeSplash)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    assert payload["results"][0]["result"]["id"] == "splash-1"


def test_build_and_post_offer_returns_nonzero_when_offer_verification_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1bad",
    )
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: "wallet_sdk_offer_verify_false",
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
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False


def test_build_and_post_offer_blocks_publish_when_offer_has_no_expiry(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)
    called: dict[str, bool] = {"post_offer_called": False}

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            called["post_offer_called"] = True
            return {"success": True, "id": "should-not-post"}

    from tests.helpers.kernel_mock import MinimalSignerKernel

    class _Signer(MinimalSignerKernel):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_missing_expiration"

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Signer)
    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1noexpiry",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)

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
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False
    assert payload["results"][0]["result"]["error"] == "wallet_sdk_offer_missing_expiration"
    assert called["post_offer_called"] is False


def test_build_and_post_offer_returns_nonzero_when_publish_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": False, "error": "dexie_http_error:500"}

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1abc",
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.offer_policy.verify_offer_for_dexie",
        lambda _offer: None,
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
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False


def test_build_and_post_offer_dry_run_returns_nonzero_when_build_fails(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program(program, tmp_path=tmp_path)
    write_markets(markets)

    def _raise_build_error(_payload):
        raise RuntimeError("signing_failed:no_agg_sig_targets_found")

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        _raise_build_error,
    )

    code = build_and_post_offer_cli(
        program_path=program,
        markets_path=markets,
        network="testnet11",
        market_id="m1",
        pair=None,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api-testnet.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        dry_run=True,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_attempts"] == 1
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["success"] is False
    assert payload["results"][0]["result"]["error"].startswith("offer_builder_failed:")


def test_local_offer_create_fn_delegates_to_offer_builder(monkeypatch) -> None:
    from dataclasses import replace

    from greenfloor.cli import offer_build_post
    from greenfloor.runtime.local_offer import make_local_offer_create_fn
    from tests.helpers.offer_runtime_fixtures import (
        market_config_for_local_offer,
        program_config_for_local_offer,
    )

    monkeypatch.setattr(
        "greenfloor.cli.offer_build_post.build_offer",
        lambda _payload: "offer1direct",
    )
    market = replace(
        market_config_for_local_offer(),
        pricing={"min_price_quote_per_base": 0.0031, "max_price_quote_per_base": 0.0038},
    )
    build_ctx = offer_build_post.prepare_offer_build_context(
        program=program_config_for_local_offer(),
        market=market,
        program_path=Path("/tmp/program.yaml"),
        network="mainnet",
        keyring_yaml_path="/tmp/keyring.yaml",
    )
    create_fn = make_local_offer_create_fn(
        build_ctx,
        dry_run=False,
        build_offer_fn=offer_build_post.build_offer,
    )
    outcome = create_fn(size_base_units=1, quote_price=0.5, action_side="sell")
    assert outcome.offer_text == "offer1direct"
