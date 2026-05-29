from __future__ import annotations

from typing import Any, cast

import pytest

from greenfloor.core.offer_policy import (
    bootstrap_block_error,
    dexie_offer_asset_expectation_error,
    expected_publish_asset_fields,
    resolve_offer_expiry_for_pricing,
    resolve_quote_price_for_pricing,
    verify_offer_for_dexie,
)
from greenfloor.runtime.offer_publish import (
    post_dexie_offer_with_invalid_offer_retry,
    post_offer_phase,
    verify_dexie_offer_visible_by_id,
)
from tests.helpers.engine_mock import MinimalSignerEngine, install_engine_stub


def test_verify_offer_for_dexie_success(monkeypatch) -> None:
    calls: list[str] = []

    class _Native(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(offer: str) -> None:
            calls.append(offer)

    install_engine_stub(monkeypatch, _Native)
    assert verify_offer_for_dexie("offer1ok") is None
    assert calls == ["offer1ok"]


def test_verify_offer_for_dexie_maps_duplicate_spends(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_duplicate_spent_coin_ids"

    install_engine_stub(monkeypatch, _Native)
    assert verify_offer_for_dexie("offer1duplicate") == "wallet_sdk_offer_duplicate_spent_coin_ids"


def test_verify_offer_for_dexie_maps_missing_expiration(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_missing_expiration"

    install_engine_stub(monkeypatch, _Native)
    assert verify_offer_for_dexie("offer1noexpiry") == "wallet_sdk_offer_missing_expiration"


def test_verify_offer_for_dexie_returns_native_validation_error(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_validate_failed:native_invalid_offer"

    install_engine_stub(monkeypatch, _Native)
    assert verify_offer_for_dexie("offer1bad") == (
        "wallet_sdk_offer_validate_failed:native_invalid_offer"
    )


def test_verify_offer_for_dexie_maps_structure_validate_failed(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def verify_offer_for_dexie(_offer: str) -> str:
            return "wallet_sdk_offer_validate_failed:malformed_offer"

    install_engine_stub(monkeypatch, _Native)
    assert verify_offer_for_dexie("offer1malformed") == (
        "wallet_sdk_offer_validate_failed:malformed_offer"
    )


def test_verify_offer_for_dexie_reports_missing_kernel(monkeypatch) -> None:
    monkeypatch.setattr(
        "greenfloor.core.engine_bridge.import_engine",
        lambda: (_ for _ in ()).throw(ImportError("greenfloor_engine_unavailable")),
    )
    assert verify_offer_for_dexie("offer1contract") == (
        "wallet_sdk_import_error:greenfloor_engine_unavailable"
    )


def test_bootstrap_block_error_delegates_to_kernel(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def bootstrap_block_error(
            bootstrap_status: str,
            bootstrap_reason: str,
            bootstrap_ready: bool,
        ) -> str | None:
            _ = bootstrap_ready
            return f"kernel_bootstrap:{bootstrap_status}:{bootstrap_reason}"

    install_engine_stub(monkeypatch, _Native)
    assert (
        bootstrap_block_error(
            bootstrap_status="failed",
            bootstrap_reason="split_error",
            bootstrap_ready=False,
        )
        == "kernel_bootstrap:failed:split_error"
    )


def test_bootstrap_block_error_requires_kernel_symbol(monkeypatch) -> None:
    class _Native:
        pass

    install_engine_stub(monkeypatch, _Native)
    with pytest.raises(RuntimeError, match="Missing symbol: bootstrap_block_error"):
        bootstrap_block_error(
            bootstrap_status="executed",
            bootstrap_reason="split_submitted",
            bootstrap_ready=False,
        )


def test_expected_publish_asset_fields_delegates_to_kernel(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def expected_publish_asset_fields(
            side: str,
            base_symbol: str,
            quote_asset: str,
            resolved_base_asset_id: str,
            resolved_quote_asset_id: str,
        ) -> dict[str, str]:
            return {
                "expected_offered_asset_id": f"offered:{side}:{resolved_quote_asset_id}",
                "expected_offered_symbol": f"{quote_asset}-{base_symbol}",
                "expected_requested_asset_id": f"requested:{resolved_base_asset_id}",
                "expected_requested_symbol": base_symbol,
            }

    install_engine_stub(monkeypatch, _Native)
    assert expected_publish_asset_fields(
        side="buy",
        base_symbol="A1",
        quote_asset="xch",
        resolved_base_asset_id="base",
        resolved_quote_asset_id="quote",
    ) == {
        "expected_offered_asset_id": "offered:buy:quote",
        "expected_offered_symbol": "xch-A1",
        "expected_requested_asset_id": "requested:base",
        "expected_requested_symbol": "A1",
    }


def test_expected_publish_asset_fields_requires_kernel_symbol(monkeypatch) -> None:
    class _Native:
        pass

    install_engine_stub(monkeypatch, _Native)
    with pytest.raises(RuntimeError, match="Missing symbol: expected_publish_asset_fields"):
        expected_publish_asset_fields(
            side="buy",
            base_symbol="A1",
            quote_asset="xch",
            resolved_base_asset_id="base",
            resolved_quote_asset_id="quote",
        )


def test_expected_publish_asset_fields_requires_complete_payload(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def expected_publish_asset_fields(
            _side: str,
            _base_symbol: str,
            _quote_asset: str,
            _resolved_base_asset_id: str,
            _resolved_quote_asset_id: str,
        ) -> dict[str, str]:
            return {
                "expected_offered_asset_id": "offered",
                "expected_requested_asset_id": "requested",
                "expected_requested_symbol": "A1",
            }

    install_engine_stub(monkeypatch, _Native)
    with pytest.raises(
        TypeError,
        match="expected_publish_asset_fields missing keys: expected_offered_symbol",
    ):
        expected_publish_asset_fields(
            side="buy",
            base_symbol="A1",
            quote_asset="xch",
            resolved_base_asset_id="base",
            resolved_quote_asset_id="quote",
        )


def test_resolve_offer_expiry_and_quote_price_use_kernel(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def resolve_offer_expiry_for_pricing(_pricing):
            return ("minutes", 12)

        @staticmethod
        def resolve_quote_price_for_pricing(_pricing):
            return 1.5

    install_engine_stub(monkeypatch, _Native)
    pricing = {"strategy_offer_expiry_minutes": 12}
    assert resolve_offer_expiry_for_pricing(pricing) == ("minutes", 12)
    assert resolve_quote_price_for_pricing(pricing) == 1.5


def test_dexie_offer_asset_expectation_error_delegates_to_kernel(monkeypatch) -> None:
    class _Native(MinimalSignerEngine):
        @staticmethod
        def dexie_offer_asset_expectation_error(
            offered: object,
            requested: object,
            expected_offered_asset_id: str,
            expected_offered_symbol: str,
            expected_requested_asset_id: str,
            expected_requested_symbol: str,
        ) -> str | None:
            _ = offered, requested
            return (
                "dexie_offer_requested_asset_missing:"
                f"expected_asset={expected_requested_asset_id}:"
                f"expected_symbol={expected_requested_symbol}:"
                f"offered={expected_offered_asset_id}:"
                f"offered_symbol={expected_offered_symbol}"
            )

    install_engine_stub(monkeypatch, _Native)
    assert dexie_offer_asset_expectation_error(
        offered=[],
        requested=[],
        expected_offered_asset_id="offer-asset",
        expected_offered_symbol="offer-symbol",
        expected_requested_asset_id="request-asset",
        expected_requested_symbol="request-symbol",
    ) == (
        "dexie_offer_requested_asset_missing:expected_asset=request-asset:"
        "expected_symbol=request-symbol:offered=offer-asset:offered_symbol=offer-symbol"
    )


def test_dexie_offer_asset_expectation_error_requires_kernel_symbol(monkeypatch) -> None:
    class _Native:
        pass

    install_engine_stub(monkeypatch, _Native)
    with pytest.raises(RuntimeError, match="Missing symbol: dexie_offer_asset_expectation_error"):
        dexie_offer_asset_expectation_error(
            offered=[],
            requested=[],
            expected_offered_asset_id="offer-asset",
            expected_offered_symbol="offer-symbol",
            expected_requested_asset_id="request-asset",
            expected_requested_symbol="request-symbol",
        )


def test_verify_dexie_offer_visible_by_id_uses_kernel_asset_expectation(monkeypatch) -> None:
    class _Dexie:
        @staticmethod
        def get_offer(_offer_id: str) -> dict[str, object]:
            return {
                "offer": {
                    "id": "offer-123",
                    "offered": [{"id": "xch"}],
                    "requested": [{"id": "cat"}],
                }
            }

    monkeypatch.setattr(
        "greenfloor.core.offer_policy.dexie_offer_asset_expectation_error",
        lambda **_kwargs: "dexie_offer_offered_asset_missing:expected_asset=abc:expected_symbol=A",
    )

    error = verify_dexie_offer_visible_by_id(
        dexie=cast(Any, _Dexie()),
        offer_id="offer-123",
        expected_offered_asset_id="abc",
        expected_offered_symbol="A",
        expected_requested_asset_id="def",
        expected_requested_symbol="B",
        sleep_fn=lambda _seconds: None,
    )
    assert error == "dexie_offer_offered_asset_missing:expected_asset=abc:expected_symbol=A"


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
