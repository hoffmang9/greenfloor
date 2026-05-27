from __future__ import annotations

import datetime as dt
import json
from dataclasses import replace
from pathlib import Path

from greenfloor.config.io import load_program_config
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.offer_execution import build_and_post_offer_cloud_wallet
from tests.helpers.cloud_wallet_offer_deps import cloud_wallet_test_deps
from tests.helpers.config_fixtures import minimal_market_config
from tests.helpers.offer_runtime_fixtures import (
    load_program_and_market,
    offer_build_context_for_program_market,
    write_manager_program,
    write_manager_program_with_cloud_wallet,
    write_markets_with_ladder,
)


def test_build_and_post_offer_cloud_wallet_fails_when_dexie_offer_not_visible(
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
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwvisibility",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=100,
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

    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    assert "dexie_get_offer_error" in payload["results"][0]["result"]["error"]


def test_build_and_post_offer_cloud_wallet_fails_when_dexie_visible_offer_size_mismatches(
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
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1cwmismatch",
        verify_offer_text_for_dexie_fn=lambda _offer: None,
        dexie_adapter_cls=_FakeDexie,
    )
    code, _ = build_and_post_offer_cloud_wallet(
        deps=deps,
        size_base_units=100,
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

    assert code == 2
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 1
    assert "dexie_offer_offered_asset_missing" in payload["results"][0]["result"]["error"]


def test_build_and_post_offer_cloud_wallet_returns_error_when_no_offer_artifact(
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
            _ = split_input_coins, split_input_coins_fee
            return {"signature_request_id": "sr-1", "status": "UNSIGNED"}

        @staticmethod
        def get_wallet(*, is_creator=None, states=None, first=100):
            return {"offers": []}  # no offer1... bech32

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
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: (_ for _ in ()).throw(
            RuntimeError("cloud_wallet_offer_artifact_timeout")
        ),
        verify_offer_text_for_dexie_fn=lambda _offer: None,
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
        cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1badoffer",
        verify_offer_text_for_dexie_fn=lambda _offer: "wallet_sdk_offer_missing_expiration",
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
    assert code == 2
    assert post_called[0] is False
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["results"][0]["result"]["error"] == "wallet_sdk_offer_missing_expiration"


def test_build_and_post_offer_cloud_wallet_passes_min_created_at_to_artifact_poll(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program_path = tmp_path / "program.yaml"
    write_manager_program(program_path, tmp_path=tmp_path, provider="dexie")
    market = replace(
        minimal_market_config(),
        base_asset="4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7",
        quote_asset="wUSDC.b",
        base_symbol="ECO.181.2022",
        pricing={"fixed_quote_per_base": 7.75, "base_unit_mojo_multiplier": 1000},
        receive_address="xch1test",
    )

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

    program = load_program_config(program_path)
    program.home_dir = str(tmp_path)
    deps = cloud_wallet_test_deps(
        wallet_factory=lambda _p: _FakeWallet(),
        resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
        ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
            "status": "skipped",
            "reason": "already_ready",
        },
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
        resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
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
            program=program,
            market=market,
            program_path=program_path,
        ),
        dry_run=False,
    )
    assert code == 0
    assert poll_calls
    assert isinstance(poll_calls[0].get("min_created_at"), dt.datetime)
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["publish_failures"] == 0
