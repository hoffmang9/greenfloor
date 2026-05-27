from __future__ import annotations

import datetime as dt
import json
from dataclasses import replace
from pathlib import Path
from typing import Any, cast

import greenfloor.cli.manager as manager_mod
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cli.manager import _build_and_post_offer
from greenfloor.runtime.cloud_wallet.bootstrap import ensure_offer_bootstrap_denominations
from greenfloor.runtime.cloud_wallet.deps import default_cloud_wallet_offer_deps
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.offer_execution import build_and_post_offer_cloud_wallet

from tests.helpers.offer_runtime_fixtures import (
    write_markets,
    write_markets_with_duplicate_pair,
    write_markets_with_ladder,
    write_program,
    write_program_with_cloud_wallet,
)

from tests.helpers.cloud_wallet_offer_deps import cloud_wallet_test_deps
from tests.logging_helpers import reset_concurrent_log_handlers

from tests.helpers.offer_runtime_fixtures import (
    write_markets,
    write_markets_with_duplicate_pair,
    write_markets_with_ladder,
    write_program,
    write_program_with_cloud_wallet,
)


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

