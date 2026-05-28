from __future__ import annotations

import sys
from typing import Any, cast

from greenfloor.core.offer_policy import (
    resolve_offer_expiry_for_pricing,
    resolve_quote_price_for_pricing,
)
from greenfloor.runtime.offer_publish import (
    post_dexie_offer_with_invalid_offer_retry,
    post_offer_phase,
    verify_offer_text_for_dexie,
)
from tests.helpers.kernel_mock import MinimalSignerKernel


def test_verify_offer_text_for_dexie_success(monkeypatch) -> None:
    calls: list[str] = []

    class _Native(MinimalSignerKernel):
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls.append(offer)

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    assert verify_offer_text_for_dexie("offer1ok") is None
    assert calls == ["offer1ok"]


def test_verify_offer_text_for_dexie_maps_duplicate_spends(monkeypatch) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_duplicate_spent_coin_ids"

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    assert (
        verify_offer_text_for_dexie("offer1duplicate")
        == "wallet_sdk_offer_duplicate_spent_coin_ids"
    )


def test_verify_offer_text_for_dexie_maps_missing_expiration(monkeypatch) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_missing_expiration"

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    assert (
        verify_offer_text_for_dexie("offer1noexpiry") == "wallet_sdk_offer_missing_expiration"
    )


def test_verify_offer_text_for_dexie_returns_native_validation_error(monkeypatch) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_validate_failed:native_invalid_offer"

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    assert verify_offer_text_for_dexie("offer1bad") == (
        "wallet_sdk_offer_validate_failed:native_invalid_offer"
    )


def test_verify_offer_text_for_dexie_reports_missing_kernel(monkeypatch) -> None:
    def _import_kernel():
        raise ImportError("greenfloor_signer_unavailable")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.verify_offer_for_dexie",
        lambda _offer: (_ for _ in ()).throw(ImportError("greenfloor_signer_unavailable")),
    )
    assert verify_offer_text_for_dexie("offer1contract") == (
        "wallet_sdk_import_error:greenfloor_signer_unavailable"
    )


def test_resolve_offer_expiry_and_quote_price_use_kernel(monkeypatch) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def resolve_offer_expiry_for_pricing(_pricing):
            return ("minutes", 12)

        @staticmethod
        def resolve_quote_price_for_pricing(_pricing):
            return 1.5

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    pricing = {"strategy_offer_expiry_minutes": 12}
    assert resolve_offer_expiry_for_pricing(pricing) == ("minutes", 12)
    assert resolve_quote_price_for_pricing(pricing) == 1.5


def test_post_dexie_offer_with_invalid_offer_retry_recovers(monkeypatch) -> None:
    calls: list[int] = []
    sleep_calls: list[float] = []

    class _FakeDexie:
        @staticmethod
        def post_offer(_offer, *, drop_only=True, claim_rewards=False):
            _ = drop_only, claim_rewards
            calls.append(1)
            if len(calls) == 1:
                return {
                    "success": False,
                    "error": 'dexie_http_error:400:{"success":false,"error_message":"Invalid Offer"}',
                }
            return {"success": True, "id": "offer-1"}

    result = post_dexie_offer_with_invalid_offer_retry(
        dexie=_FakeDexie(),  # type: ignore[arg-type]
        offer_text="offer1abc",
        drop_only=True,
        claim_rewards=False,
        sleep_fn=lambda s: sleep_calls.append(s),
    )
    assert result["success"] is True
    assert len(calls) == 2
    assert sleep_calls == [1.0]


def test_post_dexie_offer_with_invalid_offer_retry_exhausts(monkeypatch) -> None:
    calls: list[int] = []
    sleep_calls: list[float] = []

    class _FakeDexie:
        @staticmethod
        def post_offer(_offer, *, drop_only=True, claim_rewards=False):
            _ = drop_only, claim_rewards
            calls.append(1)
            return {
                "success": False,
                "error": 'dexie_http_error:400:{"success":false,"error_message":"Invalid Offer"}',
            }

    result = post_dexie_offer_with_invalid_offer_retry(
        dexie=_FakeDexie(),  # type: ignore[arg-type]
        offer_text="offer1abc",
        drop_only=True,
        claim_rewards=False,
        sleep_fn=lambda s: sleep_calls.append(s),
    )
    assert result["success"] is False
    assert len(calls) == 4
    assert sleep_calls == [1.0, 2.0, 4.0]


def test_cloud_wallet_post_offer_phase_verifies_dexie_visibility(monkeypatch) -> None:
    class _Dexie:
        pass

    dexie = _Dexie()
    result = post_offer_phase(
        publish_venue="dexie",
        dexie=cast(Any, dexie),
        splash=None,
        offer_text="offer1abc",
        drop_only=False,
        claim_rewards=False,
        expected_offered_asset_id="asset",
        expected_offered_symbol="asset",
        expected_requested_asset_id="xch",
        expected_requested_symbol="xch",
        post_dexie_offer_with_invalid_offer_retry_fn=lambda **_kwargs: {
            "success": True,
            "id": "offer-1",
        },
        verify_dexie_offer_visible_by_id_fn=lambda **_kwargs: (
            "dexie_offer_not_visible_after_publish"
        ),
    )
    assert result["success"] is False
    assert "dexie_offer_not_visible_after_publish" in str(result["error"])


def test_cloud_wallet_post_offer_phase_fails_after_repeated_transient_dexie_404(
    monkeypatch,
) -> None:
    class _Dexie:
        pass

    dexie = _Dexie()
    post_calls = {"count": 0}
    result = post_offer_phase(
        publish_venue="dexie",
        dexie=cast(Any, dexie),
        splash=None,
        offer_text="offer1abc",
        drop_only=False,
        claim_rewards=False,
        expected_offered_asset_id="asset",
        expected_offered_symbol="asset",
        expected_requested_asset_id="xch",
        expected_requested_symbol="xch",
        post_dexie_offer_with_invalid_offer_retry_fn=lambda **_kwargs: (
            post_calls.__setitem__("count", post_calls["count"] + 1)
            or {"success": True, "id": "offer-1"}
        ),
        verify_dexie_offer_visible_by_id_fn=lambda **_kwargs: (
            "dexie_get_offer_error:HTTP Error 404: Not Found"
        ),
        sleep_fn=lambda _seconds: None,
    )
    assert result["success"] is False
    assert "404" in str(result["error"])
    assert post_calls["count"] == 3
