from __future__ import annotations

import pytest

from greenfloor.runtime.coinset_runtime import (
    CoinsetFeeLookupPreflightError,
)
from greenfloor.runtime.coinset_runtime import (
    _resolve_taker_or_coin_operation_fee as resolve_taker_or_coin_operation_fee,
)


def test_resolve_taker_or_coin_operation_fee_uses_coinset_value(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [5, 15]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 15

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 15
    assert source == "coinset_conservative"


def test_resolve_taker_or_coin_operation_fee_applies_minimum_floor(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [2]}

        @staticmethod
        def get_conservative_fee_estimate():
            return 2

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=5,
    )
    assert fee == 5
    assert source == "coinset_conservative_minimum_floor"


def test_resolve_taker_or_coin_operation_fee_falls_back_to_config_minimum(monkeypatch) -> None:
    class _FakeCoinset:
        _calls = 0

        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": True, "estimates": [0]}

        @classmethod
        def get_conservative_fee_estimate(cls):
            cls._calls += 1
            if cls._calls == 1:
                return 1
            return None

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    monkeypatch.setenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "1")
    monkeypatch.setattr("time.sleep", lambda _seconds: None)

    fee, source = resolve_taker_or_coin_operation_fee(
        network="mainnet",
        minimum_fee_mojos=0,
    )
    assert fee == 0
    assert source == "config_minimum_fee_fallback"


def test_resolve_taker_or_coin_operation_fee_fails_on_endpoint_preflight(monkeypatch) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            raise RuntimeError("coinset_network_error:timed_out")

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "endpoint_validation_failed"
        assert "coinset_network_error" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_taker_or_coin_operation_fee_fails_on_temporary_advice_unavailable(
    monkeypatch,
) -> None:
    class _FakeCoinset:
        def __init__(self, _arg, *, network: str):
            self._network = network

        @staticmethod
        def get_fee_estimate(*, target_times=None):
            _ = target_times
            return {"success": False, "error": "backend_overloaded"}

    monkeypatch.setattr("greenfloor.runtime.coinset_runtime.CoinsetAdapter", _FakeCoinset)
    try:
        resolve_taker_or_coin_operation_fee(network="mainnet", minimum_fee_mojos=0)
    except CoinsetFeeLookupPreflightError as exc:
        assert exc.failure_kind == "temporary_fee_advice_unavailable"
        assert "backend_overloaded" in exc.detail
    else:
        raise AssertionError("expected _CoinsetFeeLookupPreflightError")


def test_resolve_maker_offer_fee_is_zero() -> None:
    from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee

    fee, source = resolve_maker_offer_fee(network="mainnet")
    assert fee == 0
    assert source == "maker_default_zero"


@pytest.mark.skip(
    reason="coin op fee integration requires signer wallet mocking unavailable via native subprocess"
)
def test_coin_split_no_wait_uses_advised_fee() -> None:
    pass


@pytest.mark.skip(
    reason="coin op fee integration requires signer wallet mocking unavailable via native subprocess"
)
def test_coin_combine_no_wait_uses_advised_fee() -> None:
    pass


@pytest.mark.skip(
    reason="coin op fee integration requires signer wallet mocking unavailable via native subprocess"
)
def test_coin_split_returns_structured_error_when_fee_resolution_fails() -> None:
    pass


@pytest.mark.skip(
    reason="coin op fee integration requires signer wallet mocking unavailable via native subprocess"
)
def test_coin_combine_returns_structured_error_when_fee_resolution_fails() -> None:
    pass


@pytest.mark.skip(
    reason="coin op fee integration requires signer wallet mocking unavailable via native subprocess"
)
def test_coin_combine_distinguishes_temporary_fee_advice_unavailability() -> None:
    pass
