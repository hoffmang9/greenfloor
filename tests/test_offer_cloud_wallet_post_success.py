from __future__ import annotations

import datetime as dt
import json
from pathlib import Path

import greenfloor.cli.offer_build_post as offer_build_post_mod
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
)
from greenfloor.runtime.offer_execution import build_and_post_offer_cloud_wallet
from greenfloor.runtime.offer_publish import initialize_manager_file_logging
from tests.helpers.cloud_wallet_offer_deps import cloud_wallet_test_deps
from tests.helpers.offer_runtime_fixtures import (
    load_program_and_market,
    offer_build_context_for_program_market,
    write_manager_program_with_cloud_wallet,
    write_markets_with_ladder,
)
from tests.logging_helpers import reset_concurrent_log_handlers


def test_build_and_post_offer_cloud_wallet_happy_path_dexie(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program_path, tmp_path=tmp_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)
    prog.app_log_level = "DEBUG"
    reset_concurrent_log_handlers(module=offer_build_post_mod)
    initialize_manager_file_logging(str(tmp_path), log_level="DEBUG")

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1testartifact"}]}

        @staticmethod
        def list_coins(*, asset_id: str, include_pending: bool = True):
            _ = asset_id, include_pending
            return [{"id": f"coin-{i}", "amount": "10000", "state": "CONFIRMED"} for i in range(5)]

        @staticmethod
        def split_coins(**kwargs):
            _ = kwargs
            return {"signature_request_id": "sr-split", "status": "SUBMITTED"}

    posted = {}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            posted["offer"] = offer
            return {"success": True, "id": "dexie-99"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {"success": True, "offer": {"id": str(offer_id), "status": 0}}

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
            "status": "skipped",
            "reason": "already_ready",
        },
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1testartifact",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=offer_build_context_for_program_market(
            program=prog,
            market=mkt,
            program_path=program_path,
        ),
        dry_run=False,
    )
    assert code == 0
    assert posted["offer"] == "offer1testartifact"
    captured = capsys.readouterr()
    payload = json.loads(captured.out.strip())
    assert payload["publish_failures"] == 0
    assert payload["results"][0]["result"]["id"] == "dexie-99"
    assert (
        payload["results"][0]["result"]["offer_view_url"] == "https://dexie.space/offers/dexie-99"
    )
    assert payload["offer_fee_mojos"] == 0
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
    event_items = list((events[0].get("payload") or {}).get("items") or [])
    assert len(event_items) == 1
    assert event_items[0]["side"] == "sell"
    assert captured.err == ""
    log_text = (tmp_path / "logs" / "debug.log").read_text(encoding="utf-8")
    assert "signed_offer_file:offer1testartifact" in log_text
    assert "signed_offer_metadata:ticker=A1" in log_text
    assert "amount=10" in log_text
    assert "trading_pair=A1:xch" in log_text


def test_build_and_post_offer_cloud_wallet_uses_market_configured_expiry_override(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program_path, tmp_path=tmp_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)
    pricing = dict(mkt.pricing or {})
    pricing["strategy_offer_expiry_minutes"] = 12
    mkt.pricing = pricing

    captured_expires: dict[str, str] = {}

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = offered, requested, fee, split_input_coins, split_input_coins_fee
            captured_expires["iso"] = str(expires_at_iso)
            return {"signature_request_id": "sr-expiry-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1cwexpiry"}]}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        @staticmethod
        def post_offer(_offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "dexie-expiry-1"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {"success": True, "offer": {"id": str(offer_id), "status": 0}}

    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.phases.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
            "status": "skipped",
            "reason": "already_ready",
        },
        cloud_wallet_create_offer_phase_fn=cloud_wallet_create_offer_phase,
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwexpiry",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=offer_build_context_for_program_market(
            program=prog,
            market=mkt,
            program_path=program_path,
        ),
        dry_run=False,
    )
    assert code == 0
    assert "iso" in captured_expires
    expires_at = dt.datetime.fromisoformat(captured_expires["iso"])
    now = dt.datetime.now(dt.UTC)
    delta_seconds = (expires_at - now).total_seconds()
    assert delta_seconds > 10 * 60
    assert delta_seconds < 14 * 60
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0


def test_build_and_post_offer_cloud_wallet_records_buy_side_in_audit_event(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program_path, tmp_path=tmp_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = offered, requested, fee, expires_at_iso, split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-buy-audit-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1buyaudit"}]}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        @staticmethod
        def post_offer(_offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "dexie-buy-audit-1"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {"success": True, "offer": {"id": str(offer_id), "status": 0}}

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
            "status": "skipped",
            "reason": "already_ready",
        },
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1buyaudit",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=offer_build_context_for_program_market(
            program=prog,
            market=mkt,
            program_path=program_path,
            action_side="buy",
        ),
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
    event_items = list((events[0].get("payload") or {}).get("items") or [])
    assert len(event_items) == 1
    assert event_items[0]["side"] == "buy"


def test_build_and_post_offer_cloud_wallet_dry_run_skips_publish(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program_path, tmp_path=tmp_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = offered, requested, fee, expires_at_iso, split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1dryruncloudwallet"}]}

    class _FailDexie:
        def __init__(self, _base_url: str):
            raise AssertionError("DexieAdapter must not be constructed in dry_run")

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
            "status": "skipped",
            "reason": "already_ready",
        },
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1dryruncloudwallet",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FailDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=offer_build_context_for_program_market(
            program=prog,
            market=mkt,
            program_path=program_path,
        ),
        dry_run=True,
    )
    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["dry_run"] is True
    assert payload["publish_attempts"] == 0
    assert payload["publish_failures"] == 0
    assert payload["results"] == []
    assert len(payload["built_offers_preview"]) == 1


def test_build_and_post_offer_cloud_wallet_uses_bootstrap_fallback_split_fee(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_manager_program_with_cloud_wallet(program_path, tmp_path=tmp_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)

    create_offer_calls: list[int] = []

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def __init__(self, _config):
            pass

        @staticmethod
        def create_offer(
            *,
            offered,
            requested,
            fee,
            expires_at_iso,
            split_input_coins=True,
            split_input_coins_fee=0,
        ):
            _ = offered, requested, fee, expires_at_iso, split_input_coins
            create_offer_calls.append(int(split_input_coins_fee))
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1bootstrapfee"}]}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        @staticmethod
        def post_offer(_offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "dexie-bootstrap-1"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {"success": True, "offer": {"id": str(offer_id), "status": 0}}

    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.phases.poll_signature_request_until_not_unsigned",
        lambda **kwargs: ("SUBMITTED", []),
    )

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **_kwargs: {
            "status": "failed",
            "reason": "bootstrap_failed_for_test",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": 123,
        },
        cloud_wallet_create_offer_phase_fn=cloud_wallet_create_offer_phase,
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1bootstrapfee",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=offer_build_context_for_program_market(
            program=prog,
            market=mkt,
            program_path=program_path,
        ),
        dry_run=False,
    )

    assert code == 0
    assert create_offer_calls == [0]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["bootstrap_actions"][0]["status"] == "failed"
