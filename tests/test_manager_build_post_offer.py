from __future__ import annotations

import json
from pathlib import Path

import yaml

from greenfloor.cli.offer_build_post import build_and_post_offer_cli
from tests.helpers.fake_adapters import FakeDexie
from tests.helpers.offer_runtime_fixtures import (
    patch_signer_create_offer_phase,
    write_manager_program_with_signer,
    write_markets,
    write_markets_with_duplicate_pair,
)


def test_build_and_post_offer_defaults_to_mainnet(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    raw = yaml.safe_load(markets.read_text(encoding="utf-8"))
    pricing = dict(raw["markets"][0].get("pricing") or {})
    pricing["strategy_offer_expiry_minutes"] = 12
    raw["markets"][0]["pricing"] = pricing
    markets.write_text(yaml.safe_dump(raw, sort_keys=False), encoding="utf-8")

    captured_kwargs: dict[str, object] = {}

    class _FakeDexie(FakeDexie):
        offer_id = "offer-expiry-1"

    patch_signer_create_offer_phase(monkeypatch)

    def _capture_create(**kwargs: object):
        captured_kwargs.update(kwargs)
        from greenfloor.core.offer_action import OfferCreatePhaseOutcome

        return OfferCreatePhaseOutcome(
            offer_text="offer1expiryoverride",
            expires_at="2026-01-01T00:00:00+00:00",
            side="sell",
            offer_amount=1000,
            request_amount=1000,
            execution_mode="direct",
            create_result={},
        )

    monkeypatch.setattr(
        "greenfloor.runtime.offer_runtime.signer_create_offer_phase",
        _capture_create,
    )
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr("greenfloor.core.policy_bridge.verify_offer_for_dexie", lambda _offer: None)

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
    market = captured_kwargs["market"]
    from greenfloor.config.models import MarketConfig

    assert isinstance(market, MarketConfig)
    assert market.pricing.get("strategy_offer_expiry_minutes") == 12
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0
    assert payload["results"][0]["result"]["id"] == "offer-expiry-1"


def test_build_and_post_offer_dry_run_builds_but_does_not_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FailDexie:
        def __init__(self, _base_url: str) -> None:
            raise AssertionError("DexieAdapter should not be constructed in dry_run")

    patch_signer_create_offer_phase(monkeypatch)
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


def test_build_and_post_offer_resolves_market_by_pair(monkeypatch, tmp_path: Path, capsys) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie(FakeDexie):
        offer_id = "offer-xyz"

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie(FakeDexie):
        offer_id = "offer-txch"

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeSplash:
        def __init__(self, base_url: str) -> None:
            self.base_url = base_url

        def post_offer(self, offer: str):
            _ = offer
            return {"success": True, "id": "splash-1"}

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.SplashAdapter", _FakeSplash)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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


def test_build_and_post_offer_surfaces_stale_engine_symbol_as_user_error(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from tests.helpers.engine_mock import MinimalSignerEngine, install_engine_stub

    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _StaleEngine(MinimalSignerEngine):
        expected_publish_asset_fields = None

    install_engine_stub(monkeypatch, _StaleEngine())
    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", FakeDexie)

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
    assert payload["results"][0]["result"]["error"].startswith("offer_policy_error:")
    assert (
        "Missing symbol: expected_publish_asset_fields" in payload["results"][0]["result"]["error"]
    )


def test_build_and_post_offer_blocks_publish_when_offer_has_no_expiry(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)
    called: dict[str, bool] = {"post_offer_called": False}

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            called["post_offer_called"] = True
            return {"success": True, "id": "should-not-post"}

    from tests.helpers.engine_mock import MinimalSignerEngine, install_engine_stub

    class _Signer(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_missing_expiration"

    install_engine_stub(monkeypatch, _Signer)
    patch_signer_create_offer_phase(monkeypatch)
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    class _FakeDexie:
        def __init__(self, _base_url: str) -> None:
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            return {"success": False, "error": "dexie_http_error:500"}

    patch_signer_create_offer_phase(monkeypatch)
    monkeypatch.setattr("greenfloor.runtime.offer_orchestration.DexieAdapter", _FakeDexie)
    monkeypatch.setattr(
        "greenfloor.core.policy_bridge.verify_offer_for_dexie",
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
    write_manager_program_with_signer(program, tmp_path=tmp_path)
    write_markets(markets)

    patch_signer_create_offer_phase(
        monkeypatch,
        error="signing_failed:no_agg_sig_targets_found",
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
    assert payload["results"][0]["result"]["error"].startswith("signing_failed:")
