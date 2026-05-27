from __future__ import annotations

import datetime as dt
import json
from pathlib import Path
from typing import Any, cast

import greenfloor.cli.manager as manager_mod
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import _build_and_post_offer
from greenfloor.runtime.cloud_wallet.bootstrap import ensure_offer_bootstrap_denominations
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.offer_execution import build_and_post_offer_cloud_wallet
from tests.helpers.offer_runtime_fixtures import (
    load_program_and_market,
    write_markets,
    write_markets_with_ladder,
    write_program,
    write_program_with_cloud_wallet,
)
from tests.logging_helpers import reset_concurrent_log_handlers
from dataclasses import replace

from greenfloor.runtime.cloud_wallet.deps import default_cloud_wallet_offer_deps
from tests.helpers.cloud_wallet_offer_deps import cloud_wallet_test_deps


def test_build_and_post_offer_dispatches_to_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )

    code = _build_and_post_offer(
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


def test_build_and_post_offer_dry_run_uses_cloud_wallet_when_configured(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    dispatched = [False]
    captured_dry_run: list[bool] = []

    def _fake_cloud_wallet(**kwargs):
        dispatched[0] = True
        captured_dry_run.append(bool(kwargs["dry_run"]))
        print(json.dumps({"dry_run": True, "results": [], "built_offers_preview": []}))
        return 0, {"dry_run": True, "results": [], "built_offers_preview": []}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )

    code = _build_and_post_offer(
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


def test_build_and_post_offer_uses_local_path_for_large_size_when_cloud_wallet_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program)
    write_markets(markets)

    cloud_dispatched = [False]
    local_builder_calls = [0]

    def _fake_cloud_wallet(**kwargs):
        _ = kwargs
        cloud_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request", lambda payload: "offer1abc"
    )

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            local_builder_calls[0] += 1
            return {"success": True, "id": "local-100-id"}

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr("greenfloor.cli.manager.verify_offer_text_for_dexie", lambda _offer: None)

    code = _build_and_post_offer(
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


def test_build_and_post_offer_uses_local_path_when_cloud_wallet_not_configured(
    monkeypatch, tmp_path: Path
) -> None:
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program(program)
    write_markets(markets)

    cloud_dispatched = [False]
    local_builder_calls = [0]

    def _fake_cloud_wallet(**kwargs):
        _ = kwargs
        cloud_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_cloud_wallet",
        _fake_cloud_wallet,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request", lambda payload: "offer1abc"
    )

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = offer, drop_only, claim_rewards
            local_builder_calls[0] += 1
            return {"success": True, "id": "local-no-cw"}

    monkeypatch.setattr("greenfloor.cli.manager.DexieAdapter", _FakeDexie)
    monkeypatch.setattr("greenfloor.cli.manager.verify_offer_text_for_dexie", lambda _offer: None)

    code = _build_and_post_offer(
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


def test_build_and_post_offer_uses_signer_path_for_kms_configured(
    monkeypatch, tmp_path: Path
) -> None:
    """KMS-configured runs must use the local Rust signer path for all sizes."""
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, with_kms=True)
    write_markets(markets)

    signer_dispatched = [False]
    local_builder_calls = [0]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_signer",
        _fake_signer,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.offer_execution_backend", lambda _program, **kwargs: "signer"
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    code = _build_and_post_offer(
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


def test_build_and_post_offer_uses_signer_path_for_kms_configured_large_size(
    monkeypatch, tmp_path: Path
) -> None:
    """KMS-configured runs use signer path even for size >= 100."""
    program = tmp_path / "program.yaml"
    markets = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program, with_kms=True)
    write_markets(markets)

    signer_dispatched = [False]
    local_builder_calls = [0]

    def _fake_signer(**kwargs):
        _ = kwargs
        signer_dispatched[0] = True
        return 0, {}

    monkeypatch.setattr(
        "greenfloor.cli.manager._build_and_post_offer_signer",
        _fake_signer,
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager._build_offer_text_for_request",
        lambda payload: (
            local_builder_calls.__setitem__(0, local_builder_calls[0] + 1) or "offer1abc"
        ),
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.offer_execution_backend", lambda _program, **kwargs: "signer"
    )
    monkeypatch.setattr(
        "greenfloor.cli.manager.prepare_signer_runtime",
        lambda _program: "/tmp/signer.yaml",
    )

    code = _build_and_post_offer(
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


# ---------------------------------------------------------------------------
# _build_and_post_offer_cloud_wallet direct tests
# ---------------------------------------------------------------------------


def test_build_and_post_offer_cloud_wallet_happy_path_dexie(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    from greenfloor.storage.sqlite import SqliteStore

    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
    write_markets_with_ladder(markets_path)
    prog, mkt = load_program_and_market(program_path, markets_path)
    prog.home_dir = str(tmp_path)
    prog.app_log_level = "DEBUG"
    reset_concurrent_log_handlers(module=manager_mod)

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
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
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
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
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
    write_program_with_cloud_wallet(program_path)
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
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=cloud_wallet_create_offer_phase,
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwexpiry",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=7.75,
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
    write_program_with_cloud_wallet(program_path)
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
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
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
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
        action_side="buy",
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


def test_build_and_post_offer_cloud_wallet_fails_when_dexie_offer_not_visible(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
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
            return {"signature_request_id": "sr-visibility-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1cwvisibility"}]}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        @staticmethod
        def post_offer(_offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "dexie-missing-1"}

        @staticmethod
        def get_offer(_offer_id: str) -> dict[str, object]:
            raise RuntimeError("dexie_http_error:500")

    monkeypatch.setattr("time.sleep", lambda _seconds: None)

    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwvisibility",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=100,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=7.75,
        dry_run=False,
    )

    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    assert "dexie_get_offer_error" in payload["results"][0]["result"]["error"]


def test_build_and_post_offer_cloud_wallet_fails_when_dexie_visible_offer_size_mismatches(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
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
            return {"signature_request_id": "sr-mismatch-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1cwmismatch"}]}

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        @staticmethod
        def post_offer(_offer: str, *, drop_only: bool, claim_rewards: bool | None):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "dexie-mismatch-1"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {
                "success": True,
                "offer": {
                    "id": str(offer_id),
                    "offered": [
                        {
                            "id": "unexpected_asset",
                            "amount": 10,
                        }
                    ],
                },
            }


    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwmismatch",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=100,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=7.75,
        dry_run=False,
    )

    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    assert "dexie_offer_offered_asset_missing" in payload["results"][0]["result"]["error"]


def test_build_and_post_offer_cloud_wallet_returns_error_when_no_offer_artifact(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
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
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": []}  # no offer1... bech32


    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: (_ for _ in ()).throw(RuntimeError("cloud_wallet_offer_artifact_timeout")),
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )
    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    result = payload["results"][0]["result"]
    assert result["error"] == "cloud_wallet_offer_artifact_timeout"
    assert result["signature_request_id"] == "sr-1"
    assert result["signature_state"] == "SUBMITTED"
    assert isinstance(result["wait_events"], list)


def test_build_and_post_offer_cloud_wallet_verify_error_blocks_post(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
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
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": [{"bech32": "offer1badoffer"}]}

    post_called = [False]

    class _FakeDexie:
        def __init__(self, _base_url: str):
            pass

        def post_offer(self, offer: str, *, drop_only: bool, claim_rewards: bool | None):
            post_called[0] = True
            return {"success": True}


    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(_p),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "sr-1",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1badoffer",
        verify_offer_text_for_dexie_fn=lambda _offer: "wallet_sdk_offer_missing_expiration",
        dexie_adapter_cls=_FakeDexie,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )
    assert code == 2
    assert post_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["result"]["error"] == "wallet_sdk_offer_missing_expiration"


def test_build_and_post_offer_cloud_wallet_dry_run_skips_publish(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    markets_path = tmp_path / "markets.yaml"
    write_program_with_cloud_wallet(program_path)
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
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
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
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
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
    write_program_with_cloud_wallet(program_path)
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
        initialize_manager_file_logging_fn=lambda *a, **k: None,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=prog,
        market=mkt,
        size_base_units=10,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=0.003,
        dry_run=False,
    )

    assert code == 0
    assert create_offer_calls == [0]
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["bootstrap_actions"][0]["status"] == "failed"


def test_ensure_offer_bootstrap_denominations_surfaces_wait_error(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 1
        target_count = 2
        split_buffer_count = 0

    class _Market:
        ladders = {"sell": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "xch"

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = asset_id, include_pending
            return [{"id": "coin_big", "amount": 10, "state": "CONFIRMED"}]

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 10
        output_amounts_base_units = [1, 1]
        total_output_amount = 2
        change_amount = 8
        deficits = []

    class _Deficit:
        size_base_units = 1
        deficit_count = 2
        required_count = 2
        current_count = 0

    _plan = _Plan()
    _plan.deficits = [_Deficit()]

    result = ensure_offer_bootstrap_denominations(
        program=_Program(),
        market=_Market(),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="xch",
        resolved_quote_asset_id="wusdc",
        quote_price=0.999,
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _plan,
        resolve_bootstrap_split_fee_fn=lambda **_k: (0, "coinset_conservative", None),
        wait_for_mempool_then_confirmation_fn=lambda **_k: (_ for _ in ()).throw(
            RuntimeError("confirmation_wait_timeout")
        ),
        split_coins_fn=lambda **_kw: {"signature_request_id": "sr-1", "status": "SUBMITTED"},
        poll_signature_request_until_not_unsigned_fn=lambda **_kw: ("SUBMITTED", []),
    )
    assert result["status"] == "failed"
    assert result["reason"] == "bootstrap_wait_failed"
    assert result["wait_error"] == "confirmation_wait_timeout"
    assert result["fallback_to_cloud_wallet_offer_split"] is True


def test_ensure_offer_bootstrap_denominations_reports_fee_balance_guidance(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 1
        target_count = 2
        split_buffer_count = 0

    class _Market:
        ladders = {"sell": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "xch"

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = asset_id, include_pending
            return [{"id": "coin_big", "amount": 10, "state": "CONFIRMED"}]

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 10
        output_amounts_base_units = [1, 1]
        total_output_amount = 2
        change_amount = 8
        deficits = []

    class _Deficit:
        size_base_units = 1
        deficit_count = 2
        required_count = 2
        current_count = 0

    _plan = _Plan()
    _plan.deficits = [_Deficit()]

    def _failing_split(**_kw: Any) -> dict:
        raise RuntimeError("insufficient_xch_fee_balance_for_mixed_split:required=100:available=0")

    result = ensure_offer_bootstrap_denominations(
        program=_Program(),
        market=_Market(),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="xch",
        resolved_quote_asset_id="wusdc",
        quote_price=0.999,
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _plan,
        resolve_bootstrap_split_fee_fn=lambda **_k: (100, "coinset_conservative", None),
        split_coins_fn=_failing_split,
    )
    assert result["status"] == "failed"
    assert "insufficient_xch_fee_balance_for_mixed_split" in str(
        result.get("reason", "") or result.get("error", "")
    )


def test_ensure_offer_bootstrap_denominations_buy_waits_on_quote_asset(
    monkeypatch, tmp_path: Path
) -> None:
    keyring_path = tmp_path / "keyring.yaml"
    keyring_path.write_text("keys: []\n", encoding="utf-8")
    wait_asset_ids: list[str] = []
    list_asset_ids: list[str | None] = []

    class _Program:
        app_network = "mainnet"
        coin_ops_minimum_fee_mojos = 0
        cloud_wallet_base_url = "https://api.vault.chia.net"
        cloud_wallet_user_key_id = "k"
        cloud_wallet_private_key_pem_path = "/tmp/key.pem"
        cloud_wallet_vault_id = "Wallet_abc"
        cloud_wallet_kms_key_id = ""
        cloud_wallet_kms_region = ""
        cloud_wallet_kms_public_key_hex = ""

    class _LadderEntry:
        size_base_units = 10
        target_count = 1
        split_buffer_count = 0

    class _Market:
        ladders = {"buy": [_LadderEntry()]}
        receive_address = "xch1test"
        base_asset = "base_asset"
        quote_asset = "quote_asset"
        pricing = {"quote_unit_mojo_multiplier": 1000}

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id=None, include_pending=False):
            _ = include_pending
            list_asset_ids.append(asset_id)
            return [{"id": "coin_big", "amount": 50_000, "state": "CONFIRMED"}]

    class _Deficit:
        size_base_units = 10_000
        deficit_count = 1
        required_count = 1
        current_count = 0

    class _Plan:
        source_coin_id = "coin_big"
        source_amount = 50_000
        output_amounts_base_units = [10_000]
        total_output_amount = 10_000
        change_amount = 40_000
        deficits = [_Deficit()]

    result = ensure_offer_bootstrap_denominations(
        program=_Program(),
        market=_Market(),
        wallet=cast(CloudWalletAdapter, _Wallet()),
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        quote_price=1.0,
        action_side="buy",
        plan_bootstrap_mixed_outputs_fn=lambda **_k: _Plan(),
        resolve_bootstrap_split_fee_fn=lambda **_k: (0, "coinset_conservative", None),
        wait_for_mempool_then_confirmation_fn=lambda **kwargs: wait_asset_ids.append(
            str(kwargs.get("asset_id"))
        )
        or [],
        split_coins_fn=lambda **_kw: {"signature_request_id": "sr-1", "status": "SUBMITTED"},
        poll_signature_request_until_not_unsigned_fn=lambda **_kw: ("SUBMITTED", []),
    )
    assert result["status"] == "executed"
    assert wait_asset_ids == ["Asset_quote"]
    assert list_asset_ids[0] == "Asset_quote"


def test_build_and_post_offer_cloud_wallet_passes_min_created_at_to_artifact_poll(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    write_program(program_path, provider="dexie")
    market = type(
        "Market",
        (),
        {
            "market_id": "m1",
            "base_asset": "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7",
            "quote_asset": "wUSDC.b",
            "base_symbol": "ECO.181.2022",
            "pricing": {"fixed_quote_per_base": 7.75, "base_unit_mojo_multiplier": 1000},
            "receive_address": "xch1test",
        },
    )()

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def create_offer(self, **kwargs):
            _ = kwargs
            return {"signature_request_id": "SigReq_1", "status": "SUBMITTED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=0):
            _ = is_creator, states, first
            return {"offers": []}

    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.assets.resolve_cloud_wallet_offer_asset_ids",
        lambda **kwargs: ("Asset_base", "Asset_quote"),
    )
    monkeypatch.setattr(
        "greenfloor.runtime.coinset_runtime.resolve_maker_offer_fee", lambda **kwargs: (0, "test")
    )
    poll_calls: list[dict[str, object]] = []
    monkeypatch.setattr(
        "greenfloor.runtime.cloud_wallet.phases.poll_offer_artifact_until_available",
        lambda **kwargs: poll_calls.append(kwargs) or "offer1abc",
    )

    class _FakeDexie:
        def __init__(self, _base_url):
            pass

        @staticmethod
        def post_offer(_offer_text, *, drop_only=True, claim_rewards=False):
            _ = drop_only, claim_rewards
            return {"success": True, "id": "offer-id-1"}

        @staticmethod
        def get_offer(offer_id: str) -> dict[str, object]:
            return {"success": True, "offer": {"id": str(offer_id), "status": 0}}


    program = manager_mod.load_program_config(program_path)
    program.home_dir = str(tmp_path)
    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(),
        resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {"status": "skipped", "reason": "already_ready"},
        cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
            "known_offer_markers": set(),
            "offer_request_started_at": dt.datetime.now(dt.UTC),
            "signature_request_id": "",
            "signature_state": "SUBMITTED",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "wait_events": [],
            "side": kwargs.get("action_side", "sell"),
        },
        cloud_wallet_wait_offer_artifact_phase_fn=cloud_wallet_wait_offer_artifact_phase,
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
        initialize_manager_file_logging_fn=lambda *a, **k: None,
        resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        program=program,
        market=market,
        size_base_units=1,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=7.75,
        dry_run=False,
    )
    assert code == 0
    assert poll_calls
    assert isinstance(poll_calls[0].get("min_created_at"), dt.datetime)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0


# ---------------------------------------------------------------------------
# until_ready success path (stop_reason="ready")
# ---------------------------------------------------------------------------


def test_cloud_wallet_create_offer_phase_returns_structured_intermediate(monkeypatch) -> None:
    class _Wallet:
        def __init__(self) -> None:
            self.calls = 0

        def create_offer(self, **_kwargs):
            self.calls += 1
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

    wallet = _Wallet()
    market = type(
        "Market",
        (),
        {"pricing": {"base_unit_mojo_multiplier": 1000, "quote_unit_mojo_multiplier": 1000}},
    )()
    payload = cloud_wallet_create_offer_phase(
        wallet=cast(CloudWalletAdapter, wallet),
        market=market,
        size_base_units=3,
        quote_price=2.0,
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        offer_fee_mojos=0,
        split_input_coins_fee=0,
        expiry_unit="minutes",
        expiry_value=30,
        wallet_get_wallet_offers_fn=lambda *_args, **_kwargs: {"offers": []},
        poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: (
            "SUBMITTED",
            [{"event": "signature_wait_warning"}],
        ),
    )
    assert payload["signature_request_id"] == "sr-1"
    assert payload["signature_state"] == "SUBMITTED"
    assert payload["offer_amount"] == 3000
    assert isinstance(payload["wait_events"], list)
    assert wallet.calls == 1


def test_cloud_wallet_create_offer_phase_buy_side_swaps_offer_legs(monkeypatch) -> None:
    captured: dict[str, Any] = {}

    class _Wallet:
        def create_offer(self, **kwargs):
            captured.update(kwargs)
            return {"signature_request_id": "sr-buy", "status": "UNSIGNED"}

    market = type(
        "Market",
        (),
        {"pricing": {"base_unit_mojo_multiplier": 1000, "quote_unit_mojo_multiplier": 1000}},
    )()
    payload = cloud_wallet_create_offer_phase(
        wallet=cast(CloudWalletAdapter, _Wallet()),
        market=market,
        size_base_units=10,
        quote_price=0.999,
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        offer_fee_mojos=0,
        split_input_coins_fee=0,
        expiry_unit="minutes",
        expiry_value=30,
        action_side="buy",
        wallet_get_wallet_offers_fn=lambda *_args, **_kwargs: {"offers": []},
        poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: ("SUBMITTED", []),
    )
    assert payload["side"] == "buy"
    assert captured["offered"] == [{"assetId": "Asset_quote", "amount": 9990}]
    assert captured["requested"] == [{"assetId": "Asset_base", "amount": 10000}]
