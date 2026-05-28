from __future__ import annotations

import sys
from typing import Any, cast

from greenfloor.runtime.offer_publish import (
    post_dexie_offer_with_invalid_offer_retry,
    post_offer_phase,
    verify_offer_text_for_dexie,
)
from tests.helpers.kernel_mock import MinimalSignerKernel


def test_verify_offer_text_for_dexie_uses_validate_offer_when_available(monkeypatch) -> None:
    def _import_kernel():
        raise ImportError("disable native path for this test")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.import_kernel",
        _import_kernel,
    )

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_seconds_relative():
            return object()

    class _OutputValue:
        @staticmethod
        def to_list():
            return [_ConditionWithExpiry()]

    class _Output:
        value = _OutputValue()

    class _Program:
        @staticmethod
        def run(_solution, _max_cost: int, _mempool_mode: bool):
            return _Output()

    class _Clvm:
        @staticmethod
        def deserialize(_blob: bytes):
            return _Program()

    class _CoinSpendWithExpiry:
        puzzle_reveal = b"puzzle"
        solution = b"solution"

    class _SpendBundleWithExpiry:
        coin_spends = [_CoinSpendWithExpiry()]

    class _Sdk:
        Clvm = _Clvm

        @staticmethod
        def validate_offer(offer: str) -> None:
            assert offer == "offer1ok"

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithExpiry()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert verify_offer_text_for_dexie("offer1ok") is None


def test_verify_offer_text_for_dexie_falls_back_to_verify_offer(monkeypatch) -> None:
    def _import_kernel():
        raise ImportError("disable native path for this test")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.import_kernel",
        _import_kernel,
    )

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_height_absolute():
            return object()

    class _OutputValue:
        @staticmethod
        def to_list():
            return [_ConditionWithExpiry()]

    class _Output:
        value = _OutputValue()

    class _Program:
        @staticmethod
        def run(_solution, _max_cost: int, _mempool_mode: bool):
            return _Output()

    class _Clvm:
        @staticmethod
        def deserialize(_blob: bytes):
            return _Program()

    class _CoinSpendWithExpiry:
        puzzle_reveal = b"puzzle"
        solution = b"solution"

    class _SpendBundleWithExpiry:
        coin_spends = [_CoinSpendWithExpiry()]

    class _Sdk:
        Clvm = _Clvm

        @staticmethod
        def verify_offer(offer: str) -> bool:
            return offer == "offer1ok"

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithExpiry()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert verify_offer_text_for_dexie("offer1ok") is None
    assert verify_offer_text_for_dexie("offer1bad") == "wallet_sdk_offer_verify_false"


def test_verify_offer_text_for_dexie_rejects_offer_without_expiration_condition(
    monkeypatch,
) -> None:
    def _import_kernel():
        raise ImportError("disable native path for this test")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.import_kernel",
        _import_kernel,
    )

    class _ConditionNoExpiry:
        @staticmethod
        def parse_assert_before_seconds_relative():
            return None

        @staticmethod
        def parse_assert_before_seconds_absolute():
            return None

        @staticmethod
        def parse_assert_before_height_relative():
            return None

        @staticmethod
        def parse_assert_before_height_absolute():
            return None

    class _CoinSpendNoExpiry:
        @staticmethod
        def conditions():
            return [_ConditionNoExpiry()]

    class _SpendBundleNoExpiry:
        coin_spends = [_CoinSpendNoExpiry()]

    class _Sdk:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            return None

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleNoExpiry()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert verify_offer_text_for_dexie("offer1noexpiry") == "wallet_sdk_offer_missing_expiration"


def test_verify_offer_text_for_dexie_extracts_expiry_from_coin_spend_program(
    monkeypatch,
) -> None:
    def _import_kernel():
        raise ImportError("disable native path for this test")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.import_kernel",
        _import_kernel,
    )

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_seconds_absolute():
            return object()

    class _OutputValue:
        @staticmethod
        def to_list():
            return [_ConditionWithExpiry()]

    class _Output:
        value = _OutputValue()

    class _Program:
        @staticmethod
        def run(_solution, _max_cost: int, _mempool_mode: bool):
            return _Output()

    class _Clvm:
        @staticmethod
        def deserialize(_blob: bytes):
            return _Program()

    class _CoinSpend:
        puzzle_reveal = b"puzzle"
        solution = b"solution"

    class _SpendBundle:
        coin_spends = [_CoinSpend()]

    class _Sdk:
        Clvm = _Clvm

        @staticmethod
        def validate_offer(_offer: str) -> None:
            return None

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundle()

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert verify_offer_text_for_dexie("offer1ok") is None


def test_verify_offer_text_for_dexie_rejects_duplicate_spent_coin_ids(
    monkeypatch,
) -> None:
    def _import_kernel():
        raise ImportError("disable native path for this test")

    monkeypatch.setattr(
        "greenfloor.runtime.offer_publish.import_kernel",
        _import_kernel,
    )

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_height_absolute():
            return object()

    class _Coin:
        def __init__(self, coin_id: str):
            self._coin_id = coin_id

        def coin_id(self):
            return self._coin_id

    class _CoinSpend:
        def __init__(self, coin_id: str):
            self.coin = _Coin(coin_id)

        @staticmethod
        def conditions():
            return [_ConditionWithExpiry()]

    class _SpendBundleWithDuplicates:
        coin_spends = [_CoinSpend("aa" * 32), _CoinSpend("aa" * 32)]

    class _Sdk:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            return None

        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithDuplicates()

        @staticmethod
        def to_hex(value):
            return str(value)

    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert (
        verify_offer_text_for_dexie("offer1duplicate")
        == "wallet_sdk_offer_duplicate_spent_coin_ids"
    )


def test_verify_offer_text_for_dexie_uses_greenfloor_signer_before_sdk(monkeypatch) -> None:
    calls = {}

    class _Native(MinimalSignerKernel):
        @staticmethod
        def validate_offer(offer: str) -> None:
            calls["offer"] = offer

    class _Sdk:
        @staticmethod
        def validate_offer(_offer: str) -> None:
            raise AssertionError("sdk path should not run when native is available")

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)

    assert verify_offer_text_for_dexie("offer1native") is None
    assert calls["offer"] == "offer1native"


def test_verify_offer_text_for_dexie_returns_native_validation_error(monkeypatch) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def validate_offer(_offer: str) -> None:
            raise ValueError("native_invalid_offer")

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    assert verify_offer_text_for_dexie("offer1bad") == (
        "wallet_sdk_offer_validate_failed:native_invalid_offer"
    )


def test_verify_offer_text_for_dexie_checks_duplicate_spends_after_native_validation(
    monkeypatch,
) -> None:
    class _Native(MinimalSignerKernel):
        @staticmethod
        def validate_offer(_offer: str) -> None:
            return None

    class _ConditionWithExpiry:
        @staticmethod
        def parse_assert_before_height_absolute():
            return object()

    class _Coin:
        def __init__(self, coin_id: str):
            self._coin_id = coin_id

        def coin_id(self):
            return self._coin_id

    class _CoinSpend:
        def __init__(self, coin_id: str):
            self.coin = _Coin(coin_id)

        @staticmethod
        def conditions():
            return [_ConditionWithExpiry()]

    class _SpendBundleWithDuplicates:
        coin_spends = [_CoinSpend("bb" * 32), _CoinSpend("bb" * 32)]

    class _Sdk:
        @staticmethod
        def decode_offer(_offer: str):
            return _SpendBundleWithDuplicates()

        @staticmethod
        def to_hex(value):
            return str(value)

    monkeypatch.setitem(sys.modules, "greenfloor_signer", _Native)
    monkeypatch.setitem(sys.modules, "chia_wallet_sdk", _Sdk)
    assert (
        verify_offer_text_for_dexie("offer1native-dupe")
        == "wallet_sdk_offer_duplicate_spent_coin_ids"
    )


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
