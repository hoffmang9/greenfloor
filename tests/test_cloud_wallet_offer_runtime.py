from __future__ import annotations

import datetime as dt
from dataclasses import replace
from pathlib import Path
from typing import Any, cast

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig, MarketLadderEntry, ProgramConfig
from greenfloor.runtime.cloud_wallet.assets import resolve_cloud_wallet_offer_asset_ids
from greenfloor.runtime.cloud_wallet.bootstrap import ensure_offer_bootstrap_denominations
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.cloud_wallet.polling import wait_for_mempool_then_confirmation
from greenfloor.runtime.offer_build_context import OfferBuildContext, prepare_offer_build_context
from greenfloor.runtime.offer_execution import build_and_post_offer_cloud_wallet
from greenfloor.runtime.offer_publish import (
    is_transient_dexie_visibility_404_error,
    post_offer_phase,
)
from tests.helpers.cloud_wallet_offer_deps import cloud_wallet_test_deps
from tests.helpers.config_fixtures import (
    minimal_market_config,
    minimal_market_with_sell_ladder,
    minimal_program_config,
)


def _cloud_wallet_test_program(tmp_path: Path) -> ProgramConfig:
    return replace(minimal_program_config(home_dir=str(tmp_path)), app_log_level="INFO")


def _cloud_wallet_test_market() -> MarketConfig:
    return replace(
        minimal_market_config(),
        base_asset="base-asset",
        quote_asset="quote-asset",
        base_symbol="BASE",
        pricing={"fixed_quote_per_base": 1.5},
    )


def _cloud_wallet_test_build_ctx(tmp_path: Path) -> OfferBuildContext:
    return prepare_offer_build_context(
        program=_cloud_wallet_test_program(tmp_path),
        market=_cloud_wallet_test_market(),
        program_path=Path(str(tmp_path)) / "program.yaml",
        network="mainnet",
        keyring_yaml_path="",
    )


def test_resolve_cloud_wallet_offer_asset_ids_maps_distinct_cat_assets(monkeypatch) -> None:
    base_cat = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    quote_cat = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_carbon",
                                    "type": "CAT2",
                                    "displayName": "ECO.181.2022",
                                    "symbol": "",
                                }
                            },
                            {
                                "node": {
                                    "assetId": "Asset_wusdc",
                                    "type": "CAT2",
                                    "displayName": "Base Warped USDC",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    def _fake_lookup_by_cat(*, canonical_cat_id_hex: str, network: str):
        _ = network
        if canonical_cat_id_hex == base_cat:
            return {"ticker_id": f"{base_cat}_xch", "base_code": "ECO.181.2022"}
        if canonical_cat_id_hex == quote_cat:
            return {"id": quote_cat, "code": "wUSDC.b", "name": "Base warp.green USDC"}
        return None

    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        _fake_lookup_by_cat,
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_symbol",
        lambda *, asset_ref, network: (
            {"id": quote_cat, "code": "wUSDC.b"} if asset_ref == "wUSDC.b" else None
        ),
    )

    base_asset, quote_asset = resolve_cloud_wallet_offer_asset_ids(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        base_asset_id=base_cat,
        quote_asset_id="wUSDC.b",
        base_symbol_hint="ECO.181.2022",
        quote_symbol_hint="wUSDC.b",
    )
    assert base_asset == "Asset_carbon"
    assert quote_asset == "Asset_wusdc"
    assert base_asset != quote_asset


def test_resolve_cloud_wallet_offer_asset_ids_uses_global_hints_without_label_match(
    monkeypatch,
) -> None:
    base_cat = "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7"
    quote_cat = "fa4a180ac326e67ea289b869e3448256f6af05721f7cf934cb9901baa6b7a99d"

    class _FakeWallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def _graphql(*, query: str, variables: dict):
            _ = query, variables
            return {
                "wallet": {
                    "assets": {
                        "edges": [
                            {
                                "node": {
                                    "assetId": "Asset_carbon",
                                    "type": "CAT2",
                                    "displayName": "Legacy Carbon Label",
                                    "symbol": "",
                                }
                            },
                            {
                                "node": {
                                    "assetId": "Asset_wusdc",
                                    "type": "CAT2",
                                    "displayName": "USD Coin",
                                    "symbol": "",
                                }
                            },
                        ]
                    }
                }
            }

    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._dexie_lookup_token_for_cat_id",
        lambda **_: None,
    )
    monkeypatch.setattr(
        "greenfloor.asset_label_catalog._local_catalog_label_hints_for_asset_id",
        lambda **_: [],
    )

    resolved_base, resolved_quote = resolve_cloud_wallet_offer_asset_ids(
        wallet=cast(CloudWalletAdapter, _FakeWallet()),
        base_asset_id=base_cat,
        quote_asset_id=quote_cat,
        base_symbol_hint="ECO.181.2022",
        quote_symbol_hint="wUSDC.b",
        base_global_id_hint="Asset_carbon",
        quote_global_id_hint="Asset_wusdc",
    )
    assert resolved_base == "Asset_carbon"
    assert resolved_quote == "Asset_wusdc"


def test_build_and_post_offer_cloud_wallet_runs_without_manager_import(tmp_path: Path) -> None:
    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

    exit_code, payload = build_and_post_offer_cloud_wallet(
        program=_cloud_wallet_test_program(tmp_path),
        market=_cloud_wallet_test_market(),
        size_base_units=5,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=_cloud_wallet_test_build_ctx(tmp_path),
        dry_run=True,
        deps=cloud_wallet_test_deps(
            wallet_factory=lambda _program: cast(CloudWalletAdapter, _Wallet()),
            recent_market_resolved_asset_id_hints_fn=lambda **kwargs: (None, None),
            resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
            resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
            ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
                "status": "skipped",
                "reason": "dry_run",
            },
            cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
                "known_offer_markers": set(),
                "offer_request_started_at": "start",
                "signature_request_id": "sr-1",
                "signature_state": "SUBMITTED",
                "wait_events": [],
                "expires_at": "2026-01-01T00:00:00+00:00",
                "offer_amount": 5000,
                "request_amount": 7500,
                "side": "sell",
            },
            cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1runtime",
            log_signed_offer_artifact_fn=lambda **kwargs: None,
            verify_offer_text_for_dexie_fn=lambda _offer_text: None,
            post_offer_phase_fn=lambda **kwargs: {"success": True, "id": "offer-1"},
            dexie_offer_view_url_fn=lambda **kwargs: "https://dexie.space/offers/offer-1",
        ),
    )

    assert exit_code == 0
    assert payload["dry_run"] is True
    assert payload["publish_failures"] == 0
    assert payload["resolved_base_asset_id"] == "Asset_base"
    assert payload["results"] == []
    assert payload["built_offers_preview"] == [
        {"offer_prefix": "offer1runtime", "offer_length": str(len("offer1runtime"))}
    ]


def test_build_and_post_offer_cloud_wallet_emits_timing_diagnostics(tmp_path: Path) -> None:
    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

    class _Dexie:
        def __init__(self, _base_url: str) -> None:
            pass

    exit_code, payload = build_and_post_offer_cloud_wallet(
        program=_cloud_wallet_test_program(tmp_path),
        market=_cloud_wallet_test_market(),
        size_base_units=5,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=_cloud_wallet_test_build_ctx(tmp_path),
        dry_run=False,
        deps=cloud_wallet_test_deps(
            wallet_factory=lambda _program: cast(CloudWalletAdapter, _Wallet()),
            dexie_adapter_cls=_Dexie,  # type: ignore[arg-type]
            recent_market_resolved_asset_id_hints_fn=lambda **kwargs: (None, None),
            resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
            resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
            ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
                "status": "skipped",
                "reason": "already_ready",
            },
            cloud_wallet_create_offer_phase_fn=lambda **kwargs: {
                "known_offer_markers": set(),
                "offer_request_started_at": "start",
                "signature_request_id": "sr-timing-1",
                "signature_state": "SUBMITTED",
                "wait_events": [],
                "expires_at": "2026-01-01T00:00:00+00:00",
                "offer_amount": 5000,
                "request_amount": 7500,
                "side": "sell",
            },
            cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1timing",
            log_signed_offer_artifact_fn=lambda **kwargs: None,
            verify_offer_text_for_dexie_fn=lambda _offer_text: None,
            post_offer_phase_fn=lambda **kwargs: {"success": True, "id": "offer-timing-1"},
            dexie_offer_view_url_fn=lambda **kwargs: "https://dexie.space/offers/offer-timing-1",
        ),
    )

    assert exit_code == 0
    timing = payload["results"][0]["result"]["timing_ms"]
    assert isinstance(timing["create_total_ms"], int)
    assert isinstance(timing["publish_ms"], int)
    assert isinstance(timing["total_ms"], int)


def test_cloud_wallet_wait_offer_artifact_phase_prefers_signature_request_lookup() -> None:
    calls = {"signature": 0, "generic": 0}

    result = cloud_wallet_wait_offer_artifact_phase(
        wallet=cast(CloudWalletAdapter, object()),
        known_markers={"id:known"},
        offer_request_started_at=dt.datetime(2026, 1, 1, tzinfo=dt.UTC),
        signature_request_id="sr-123",
        timeout_seconds=30,
        poll_offer_artifact_by_signature_request_fn=lambda **_kwargs: (
            calls.__setitem__("signature", calls["signature"] + 1) or "offer1signature"
        ),
        poll_offer_artifact_until_available_fn=lambda **_kwargs: (
            calls.__setitem__("generic", calls["generic"] + 1) or "offer1generic"
        ),
    )

    assert result == "offer1signature"
    assert calls == {"signature": 1, "generic": 0}


def test_cloud_wallet_wait_offer_artifact_phase_falls_back_after_signature_timeout() -> None:
    calls = {"signature": 0, "generic": 0}

    def _signature_poll(**_kwargs: Any) -> str:
        calls["signature"] += 1
        raise RuntimeError("cloud_wallet_offer_artifact_timeout")

    def _generic_poll(**_kwargs: Any) -> str:
        calls["generic"] += 1
        return "offer1generic"

    result = cloud_wallet_wait_offer_artifact_phase(
        wallet=cast(CloudWalletAdapter, object()),
        known_markers={"id:known"},
        offer_request_started_at=dt.datetime(2026, 1, 1, tzinfo=dt.UTC),
        signature_request_id="sr-123",
        timeout_seconds=30,
        poll_offer_artifact_by_signature_request_fn=_signature_poll,
        poll_offer_artifact_until_available_fn=_generic_poll,
    )

    assert result == "offer1generic"
    assert calls == {"signature": 2, "generic": 1}


def test_cloud_wallet_post_offer_phase_fails_after_repeated_dexie_404_visibility() -> None:
    class _Dexie:
        pass

    post_attempts: list[int] = []
    result = post_offer_phase(
        publish_venue="dexie",
        dexie=cast(Any, _Dexie()),
        splash=None,
        offer_text="offer1abc",
        drop_only=True,
        claim_rewards=False,
        expected_offered_asset_id="asset_a",
        expected_offered_symbol="A",
        expected_requested_asset_id="asset_b",
        expected_requested_symbol="B",
        post_dexie_offer_with_invalid_offer_retry_fn=lambda **_kwargs: (
            post_attempts.append(1) or {"success": True, "id": "offer-123"}
        ),
        verify_dexie_offer_visible_by_id_fn=lambda **_kwargs: (
            "dexie_get_offer_error:HTTP Error 404: Not Found"
        ),
        sleep_fn=lambda _seconds: None,
    )

    assert result["success"] is False
    assert result["id"] == "offer-123"
    assert "dexie_get_offer_error:HTTP Error 404: Not Found" in str(result["error"])
    assert len(post_attempts) == 3


def test_cloud_wallet_post_offer_phase_retries_transient_dexie_404_until_visible() -> None:
    class _Dexie:
        pass

    verify_calls = {"count": 0}

    def _verify(**_kwargs: Any) -> str | None:
        verify_calls["count"] += 1
        if verify_calls["count"] < 3:
            return "dexie_get_offer_error:HTTP Error 404: Not Found"
        return None

    result = post_offer_phase(
        publish_venue="dexie",
        dexie=cast(Any, _Dexie()),
        splash=None,
        offer_text="offer1abc",
        drop_only=True,
        claim_rewards=False,
        expected_offered_asset_id="asset_a",
        expected_offered_symbol="A",
        expected_requested_asset_id="asset_b",
        expected_requested_symbol="B",
        post_dexie_offer_with_invalid_offer_retry_fn=lambda **_kwargs: {
            "success": True,
            "id": "offer-123",
        },
        verify_dexie_offer_visible_by_id_fn=_verify,
        sleep_fn=lambda _seconds: None,
    )

    assert result == {"success": True, "id": "offer-123"}
    assert verify_calls["count"] == 3


def test_is_transient_dexie_visibility_404_error_matches_common_404_shapes() -> None:
    assert is_transient_dexie_visibility_404_error(
        "dexie_get_offer_error:HTTP Error 404: Not Found"
    )
    assert is_transient_dexie_visibility_404_error("dexie_http_error:404")
    assert not is_transient_dexie_visibility_404_error("dexie_network_error:timed out")


def test_cloud_wallet_create_offer_phase_propagates_create_offer_balance_error() -> None:
    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

        @staticmethod
        def create_offer(**_kwargs):
            raise RuntimeError(
                "cloud_wallet_offer_insufficient_spendable_balance:available=1000:required=5000"
            )

    market = replace(
        minimal_market_config(),
        pricing={
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1_000_000_000_000,
        },
    )

    try:
        cloud_wallet_create_offer_phase(
            wallet=cast(CloudWalletAdapter, _Wallet()),
            market=market,
            size_base_units=50,
            quote_price=2.94117647,
            resolved_base_asset_id="Asset_base",
            resolved_quote_asset_id="Asset_quote",
            offer_fee_mojos=0,
            split_input_coins_fee=0,
            expiry_unit="minutes",
            expiry_value=10,
            action_side="sell",
            wallet_get_wallet_offers_fn=lambda *_args, **_kwargs: {"offers": []},
            poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: ("SUBMITTED", []),
        )
        raise AssertionError("expected insufficient spendable balance error")
    except RuntimeError as exc:
        assert "cloud_wallet_offer_insufficient_spendable_balance" in str(exc)


def test_cloud_wallet_create_offer_phase_always_disables_split_input_coins_without_list_coins_precheck() -> (
    None
):
    captured: dict[str, Any] = {}
    list_coins_calls = 0

    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def list_coins(self, *, asset_id=None, include_pending=False):
            nonlocal list_coins_calls
            _ = asset_id, include_pending
            list_coins_calls += 1
            return [{"id": "coin-a", "amount": 500_000, "state": "SETTLED"}]

        @staticmethod
        def create_offer(**kwargs):
            captured.update(kwargs)
            return {"signature_request_id": "sr-1", "status": "SUBMITTED"}

    market = replace(
        minimal_market_config(),
        pricing={
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )

    payload = cloud_wallet_create_offer_phase(
        wallet=cast(CloudWalletAdapter, _Wallet()),
        market=market,
        size_base_units=1,
        quote_price=1.0,
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        offer_fee_mojos=0,
        split_input_coins_fee=123456,
        expiry_unit="minutes",
        expiry_value=10,
        action_side="sell",
        wallet_get_wallet_offers_fn=lambda *_args, **_kwargs: {"offers": []},
        poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: ("SUBMITTED", []),
    )

    assert payload["signature_request_id"] == "sr-1"
    assert captured["split_input_coins"] is False
    assert captured["split_input_coins_fee"] == 0
    assert list_coins_calls == 0


def test_cloud_wallet_create_offer_phase_does_not_call_list_coins_precheck() -> None:
    list_coins_calls = 0

    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

        def list_coins(self, *, asset_id=None, include_pending=False):
            nonlocal list_coins_calls
            _ = asset_id, include_pending
            list_coins_calls += 1
            raise RuntimeError("list_coins should not be called during create-offer phase")

        @staticmethod
        def create_offer(**_kwargs):
            return {"signature_request_id": "sr-1", "status": "SUBMITTED"}

    market = replace(
        minimal_market_config(),
        pricing={
            "base_unit_mojo_multiplier": 1000,
            "quote_unit_mojo_multiplier": 1000,
        },
    )

    payload = cloud_wallet_create_offer_phase(
        wallet=cast(CloudWalletAdapter, _Wallet()),
        market=market,
        size_base_units=1,
        quote_price=1.0,
        resolved_base_asset_id="Asset_base",
        resolved_quote_asset_id="Asset_quote",
        offer_fee_mojos=0,
        split_input_coins_fee=0,
        expiry_unit="minutes",
        expiry_value=10,
        action_side="sell",
        wallet_get_wallet_offers_fn=lambda *_args, **_kwargs: {"offers": []},
        poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: ("SUBMITTED", []),
    )
    assert payload["signature_request_id"] == "sr-1"
    assert payload["signature_state"] == "SUBMITTED"
    assert list_coins_calls == 0


def test_build_and_post_offer_cloud_wallet_skips_create_when_bootstrap_pending(
    tmp_path: Path,
) -> None:
    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

    class _Dexie:
        def __init__(self, _base_url: str) -> None:
            pass

    code, payload = build_and_post_offer_cloud_wallet(
        program=_cloud_wallet_test_program(tmp_path),
        market=_cloud_wallet_test_market(),
        size_base_units=5,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        build_ctx=_cloud_wallet_test_build_ctx(tmp_path),
        dry_run=False,
        deps=cloud_wallet_test_deps(
            wallet_factory=lambda _program: cast(CloudWalletAdapter, _Wallet()),
            dexie_adapter_cls=_Dexie,  # type: ignore[arg-type]
            recent_market_resolved_asset_id_hints_fn=lambda **kwargs: (None, None),
            resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
            resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
            ensure_offer_bootstrap_denominations_fn=lambda **kwargs: {
                "status": "executed",
                "reason": "bootstrap_submitted",
                "ready": False,
            },
            cloud_wallet_create_offer_phase_fn=lambda **_kwargs: (_ for _ in ()).throw(
                AssertionError("create phase should not run while bootstrap is pending")
            ),
            cloud_wallet_wait_offer_artifact_phase_fn=lambda **kwargs: "offer1unused",
            log_signed_offer_artifact_fn=lambda **kwargs: None,
            verify_offer_text_for_dexie_fn=lambda _offer_text: None,
            post_offer_phase_fn=lambda **kwargs: {"success": True, "id": "offer-unused"},
            dexie_offer_view_url_fn=lambda **kwargs: "https://dexie.space/offers/offer-unused",
        ),
    )

    assert code == 2
    assert payload["publish_failures"] == 1
    assert payload["results"][0]["result"]["error"].startswith("bootstrap_pending:")


def test_bootstrap_uses_cloud_wallet_split_without_keyring() -> None:
    program = replace(
        minimal_program_config(),
        coin_ops_minimum_fee_mojos=10,
        cloud_wallet_base_url="https://api.vault.chia.net",
        cloud_wallet_user_key_id="UserAuthKey_1",
        cloud_wallet_private_key_pem_path="/tmp/key.pem",
        cloud_wallet_vault_id="Wallet_1",
        cloud_wallet_kms_key_id="arn:aws:kms:us-west-2:123:key/1",
        cloud_wallet_kms_region="us-west-2",
        cloud_wallet_kms_public_key_hex="02" + ("00" * 32),
    )
    market = replace(
        minimal_market_with_sell_ladder(size_base_units=10, target_count=1),
        base_asset="cat-asset",
        quote_asset="wUSDC.b",
        receive_address="xch1test",
        pricing={},
        ladders={
            "sell": [
                MarketLadderEntry(
                    size_base_units=10,
                    target_count=1,
                    split_buffer_count=1,
                    combine_when_excess_factor=2.0,
                )
            ]
        },
    )

    class _Deficit:
        size_base_units = 10
        required_count = 2
        current_count = 0
        deficit_count = 2

    class _Plan:
        source_coin_id = "coin-1"
        source_amount = 20
        output_amounts_base_units = [10, 10]
        total_output_amount = 20
        change_amount = 0
        deficits: list[Any] = [_Deficit()]

    list_coins_include_pending_values: list[bool] = []

    class _Wallet:
        def list_coins(self, *, asset_id: str, include_pending: bool = False):
            _ = asset_id
            list_coins_include_pending_values.append(bool(include_pending))
            return [{"id": "coin-1", "amount": "20"}]

    split_calls: list[dict[str, Any]] = []

    def _fake_split_coins(**kwargs: Any) -> dict[str, Any]:
        split_calls.append(dict(kwargs))
        return {"signature_request_id": "SignatureRequest_1", "status": "SUBMITTED"}

    result = ensure_offer_bootstrap_denominations(
        program=program,
        market=market,
        wallet=cast(Any, _Wallet()),
        resolved_base_asset_id="Asset_cat",
        resolved_quote_asset_id="Asset_quote",
        quote_price=1.0,
        action_side="sell",
        plan_bootstrap_mixed_outputs_fn=lambda **_kwargs: _Plan(),
        resolve_bootstrap_split_fee_fn=lambda **_kwargs: (10, "coinset_conservative", None),
        split_coins_fn=_fake_split_coins,
        poll_signature_request_until_not_unsigned_fn=lambda **_kwargs: ("SUBMITTED", []),
        wait_for_mempool_then_confirmation_fn=lambda **_kwargs: [],
        is_spendable_coin_fn=lambda _coin: True,
    )

    assert result["status"] == "executed"
    assert result["signature_request_id"] == "SignatureRequest_1"
    assert split_calls and split_calls[0]["coin_ids"] == ["coin-1"]
    assert int(split_calls[0]["amount_per_coin"]) == 10
    assert list_coins_include_pending_values == [False, False]


def test_wait_for_mempool_then_confirmation_uses_settled_only_coin_scans() -> None:
    list_coins_include_pending_values: list[bool] = []
    list_coins_asset_ids: list[str | None] = []

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id: str | None = None, include_pending: bool = False):
            list_coins_asset_ids.append(asset_id)
            list_coins_include_pending_values.append(bool(include_pending))
            return [
                {
                    "id": "Coin_new",
                    "name": "coin-new",
                    "state": "SETTLED",
                    "asset": {"id": "Asset_test"},
                }
            ]

    events = wait_for_mempool_then_confirmation(
        wallet=cast(CloudWalletAdapter, _Wallet()),
        network="mainnet",
        initial_coin_ids=set(),
        asset_id="Asset_test",
        mempool_warning_seconds=30,
        confirmation_warning_seconds=60,
        timeout_seconds=10,
        sleep_fn=lambda _seconds: None,
        coinset_reconcile_fn=lambda **_kwargs: {
            "reconcile": "ok",
            "confirmed_block_index": "10",
        },
        reorg_watch_fn=lambda **_kwargs: [{"event": "reorg_watch_complete"}],
    )

    assert list_coins_include_pending_values == [False]
    assert list_coins_asset_ids == ["Asset_test"]
    assert any(str(event.get("event", "")).strip() == "confirmed" for event in events)


def test_wait_for_mempool_then_confirmation_uses_unscoped_scan_without_asset_id() -> None:
    list_coins_asset_ids: list[str | None] = []

    class _Wallet:
        @staticmethod
        def list_coins(*, asset_id: str | None = None, include_pending: bool = False):
            _ = include_pending
            list_coins_asset_ids.append(asset_id)
            return [
                {
                    "id": "Coin_new",
                    "name": "coin-new",
                    "state": "SETTLED",
                    "asset": {"id": "Asset_test"},
                }
            ]

    events = wait_for_mempool_then_confirmation(
        wallet=cast(CloudWalletAdapter, _Wallet()),
        network="mainnet",
        initial_coin_ids=set(),
        mempool_warning_seconds=30,
        confirmation_warning_seconds=60,
        timeout_seconds=10,
        sleep_fn=lambda _seconds: None,
        coinset_reconcile_fn=lambda **_kwargs: {
            "reconcile": "ok",
            "confirmed_block_index": "10",
        },
        reorg_watch_fn=lambda **_kwargs: [{"event": "reorg_watch_complete"}],
    )

    assert list_coins_asset_ids == [None]
    assert any(str(event.get("event", "")).strip() == "confirmed" for event in events)
