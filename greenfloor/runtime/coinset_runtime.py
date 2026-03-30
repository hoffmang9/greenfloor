"""Coinset adapter construction and fee resolution shared runtime helpers."""

from __future__ import annotations

import os
import time

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.io import is_testnet


class CoinsetFeeLookupPreflightError(RuntimeError):
    def __init__(
        self,
        *,
        failure_kind: str,
        detail: str,
        diagnostics: dict[str, str],
    ) -> None:
        self.failure_kind = failure_kind
        self.detail = detail
        self.diagnostics = diagnostics
        super().__init__(f"{failure_kind}:{detail}")


def _coinset_base_url(*, network: str) -> str:
    base = os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()
    if not base:
        return ""
    if is_testnet(network):
        allow_mainnet = os.getenv("GREENFLOOR_ALLOW_MAINNET_COINSET_FOR_TESTNET11", "").strip()
        if (
            "coinset.org" in base
            and "testnet11.api.coinset.org" not in base
            and allow_mainnet != "1"
        ):
            raise RuntimeError("coinset_base_url_mainnet_not_allowed_for_testnet11")
    return base


def _coinset_adapter(*, network: str) -> CoinsetAdapter:
    base_url = _coinset_base_url(network=network)
    require_testnet11 = is_testnet(network)
    try:
        return CoinsetAdapter(
            base_url or None, network=network, require_testnet11=require_testnet11
        )
    except TypeError as exc:
        if "require_testnet11" not in str(exc):
            raise
        return CoinsetAdapter(base_url or None, network=network)


def _coinset_fee_lookup_preflight(
    *,
    network: str,
    fee_cost: int = 1_000_000,
    spend_count: int | None = None,
) -> dict[str, str]:
    try:
        coinset = _coinset_adapter(network=network)
    except Exception as exc:
        raise CoinsetFeeLookupPreflightError(
            failure_kind="endpoint_validation_failed",
            detail=str(exc),
            diagnostics={
                "coinset_network": network.strip().lower(),
                "coinset_base_url": os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip(),
            },
        ) from exc
    diagnostics = {
        "coinset_network": str(getattr(coinset, "network", network.strip().lower())),
        "coinset_base_url": str(
            getattr(coinset, "base_url", os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip())
        ),
    }
    try:
        try:
            payload = coinset.get_fee_estimate(
                target_times=[300, 600, 1200],
                cost=max(1, int(fee_cost)),
                spend_count=spend_count,
            )
        except TypeError as exc:
            if "unexpected keyword argument" not in str(exc):
                raise
            payload = coinset.get_fee_estimate(target_times=[300, 600, 1200])
    except Exception as exc:
        raise CoinsetFeeLookupPreflightError(
            failure_kind="endpoint_validation_failed",
            detail=str(exc),
            diagnostics=diagnostics,
        ) from exc
    if not bool(payload.get("success", False)):
        detail = str(
            payload.get("error")
            or payload.get("message")
            or payload.get("reason")
            or "coinset_fee_estimate_unsuccessful"
        )
        raise CoinsetFeeLookupPreflightError(
            failure_kind="temporary_fee_advice_unavailable",
            detail=detail,
            diagnostics=diagnostics,
        )
    try:
        recommended = coinset.get_conservative_fee_estimate(
            cost=max(1, int(fee_cost)),
            spend_count=spend_count,
        )
    except TypeError as exc:
        if "unexpected keyword argument" not in str(exc):
            raise
        recommended = coinset.get_conservative_fee_estimate()
    if recommended is None:
        raise CoinsetFeeLookupPreflightError(
            failure_kind="temporary_fee_advice_unavailable",
            detail="coinset_conservative_fee_unavailable",
            diagnostics=diagnostics,
        )
    diagnostics["recommended_fee_mojos"] = str(int(recommended))
    return diagnostics


def _resolve_operation_fee(
    *,
    role: str,
    network: str,
    minimum_fee_mojos: int = 0,
    fee_cost: int = 1_000_000,
    spend_count: int | None = None,
) -> tuple[int, str]:
    if role == "maker_create_offer":
        return 0, "maker_default_zero"
    if role != "taker_or_coin_operation":
        raise ValueError(f"unsupported fee role: {role}")
    if int(minimum_fee_mojos) < 0:
        raise ValueError("minimum_fee_mojos must be >= 0")

    minimum_fee = int(minimum_fee_mojos)
    max_attempts = int(os.getenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "4"))
    coinset = _coinset_adapter(network=network)
    for attempt in range(max_attempts):
        advised = None
        try:
            try:
                advised = coinset.get_conservative_fee_estimate(
                    cost=max(1, int(fee_cost)),
                    spend_count=spend_count,
                )
            except TypeError as exc:
                if "unexpected keyword argument" not in str(exc):
                    raise
                advised = coinset.get_conservative_fee_estimate()
        except Exception:
            advised = None
        if advised is not None:
            advised_fee = int(advised)
            if advised_fee < minimum_fee:
                return minimum_fee, "coinset_conservative_minimum_floor"
            return advised_fee, "coinset_conservative"
        if attempt < max_attempts - 1:
            sleep_seconds = min(8.0, 0.5 * (2**attempt))
            time.sleep(sleep_seconds)

    return minimum_fee, "config_minimum_fee_fallback"


def _resolve_taker_or_coin_operation_fee(
    *,
    network: str,
    minimum_fee_mojos: int = 0,
    fee_cost: int = 1_000_000,
    spend_count: int | None = None,
) -> tuple[int, str]:
    _coinset_fee_lookup_preflight(
        network=network,
        fee_cost=fee_cost,
        spend_count=spend_count,
    )
    return _resolve_operation_fee(
        role="taker_or_coin_operation",
        network=network,
        minimum_fee_mojos=minimum_fee_mojos,
        fee_cost=fee_cost,
        spend_count=spend_count,
    )


def resolve_maker_offer_fee(*, network: str) -> tuple[int, str]:
    return _resolve_operation_fee(role="maker_create_offer", network=network)


_CoinsetFeeLookupPreflightError = CoinsetFeeLookupPreflightError
