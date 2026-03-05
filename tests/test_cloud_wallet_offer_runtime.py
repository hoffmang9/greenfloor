from __future__ import annotations

from pathlib import Path
from typing import cast

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cloud_wallet_offer_runtime import (
    build_and_post_offer_cloud_wallet,
    resolve_cloud_wallet_offer_asset_ids,
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
        "greenfloor.cloud_wallet_offer_runtime._dexie_lookup_token_for_cat_id",
        _fake_lookup_by_cat,
    )
    monkeypatch.setattr(
        "greenfloor.cloud_wallet_offer_runtime._dexie_lookup_token_for_symbol",
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
        "greenfloor.cloud_wallet_offer_runtime._dexie_lookup_token_for_cat_id",
        lambda **_: None,
    )
    monkeypatch.setattr(
        "greenfloor.cloud_wallet_offer_runtime._local_catalog_label_hints_for_asset_id",
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
    class _Program:
        home_dir = str(tmp_path)
        app_network = "mainnet"
        app_log_level = "INFO"

    class _Market:
        market_id = "m1"
        base_asset = "base-asset"
        quote_asset = "quote-asset"
        base_symbol = "BASE"

    class _Wallet:
        vault_id = "wallet-1"
        network = "mainnet"

    exit_code, payload = build_and_post_offer_cloud_wallet(
        program=_Program(),
        market=_Market(),
        size_base_units=5,
        repeat=1,
        publish_venue="dexie",
        dexie_base_url="https://api.dexie.space",
        splash_base_url="http://localhost:4000",
        drop_only=True,
        claim_rewards=False,
        quote_price=1.5,
        dry_run=True,
        wallet_factory=lambda _program: cast(CloudWalletAdapter, _Wallet()),
        initialize_manager_file_logging_fn=lambda *args, **kwargs: None,
        recent_market_resolved_asset_id_hints_fn=lambda **kwargs: (None, None),
        resolve_cloud_wallet_offer_asset_ids_fn=lambda **kwargs: ("Asset_base", "Asset_quote"),
        resolve_maker_offer_fee_fn=lambda **kwargs: (0, "test"),
        resolve_offer_expiry_for_market_fn=lambda _market: ("minutes", 30),
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
        cloud_wallet_post_offer_phase_fn=lambda **kwargs: {"success": True, "id": "offer-1"},
        dexie_offer_view_url_fn=lambda **kwargs: "https://dexie.space/offers/offer-1",
    )

    assert exit_code == 0
    assert payload["dry_run"] is True
    assert payload["publish_failures"] == 0
    assert payload["resolved_base_asset_id"] == "Asset_base"
    assert payload["results"] == []
    assert payload["built_offers_preview"] == [
        {"offer_prefix": "offer1runtime", "offer_length": str(len("offer1runtime"))}
    ]
