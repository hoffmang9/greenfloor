"""Venue-neutral offer logging, validation, and publish helpers."""

from __future__ import annotations

import collections.abc
import importlib
import logging
import time
import urllib.parse
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.config.models import MarketConfig
from greenfloor.hex_utils import normalize_hex_id
from greenfloor.logging_setup import initialize_service_file_logging
from greenfloor.offer_decode import (
    extract_coin_id_hints_from_offer_text as _extract_coin_id_hints_from_offer_text,
)

_MANAGER_SERVICE_NAME = "manager"
_DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS = 4
_DEXIE_INVALID_OFFER_RETRY_INITIAL_DELAY_SECONDS = 1.0
_DEXIE_VISIBILITY_POST_MAX_ATTEMPTS = 3
_DEXIE_VISIBILITY_POST_DELAY_SECONDS = 2.0
_runtime_logger = logging.getLogger("greenfloor.manager")


def initialize_manager_file_logging(home_dir: str, *, log_level: str | None) -> None:
    initialize_service_file_logging(
        service_name=_MANAGER_SERVICE_NAME,
        home_dir=home_dir,
        log_level=log_level,
        service_logger=_runtime_logger,
    )


def normalize_offer_side(value: str | None) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def dexie_offer_view_url(*, dexie_base_url: str, offer_id: str) -> str:
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return ""
    parsed = urllib.parse.urlparse(str(dexie_base_url).strip())
    host = parsed.netloc.strip().lower()
    if not host:
        return ""
    if host.startswith("api-testnet."):
        host = host[len("api-") :]
    elif host.startswith("api."):
        host = host[len("api.") :]
    return f"https://{host}/offers/{urllib.parse.quote(clean_offer_id)}"


def resolve_offer_expiry_for_market(market: MarketConfig) -> tuple[str, int]:
    pricing = dict(market.pricing or {})
    value_raw = pricing.get("strategy_offer_expiry_minutes")
    try:
        value = int(value_raw or 0)
    except (TypeError, ValueError):
        value = 0
    if value > 0:
        return "minutes", value
    return "minutes", 10


def resolve_quote_price_for_market(market: MarketConfig) -> float:
    pricing = dict(market.pricing or {})
    quote_price = pricing.get("fixed_quote_per_base")
    if quote_price is None:
        min_q = pricing.get("min_price_quote_per_base")
        max_q = pricing.get("max_price_quote_per_base")
        if min_q is not None and max_q is not None:
            quote_price = (float(min_q) + float(max_q)) / 2.0
        elif min_q is not None:
            quote_price = float(min_q)
        elif max_q is not None:
            quote_price = float(max_q)
    if quote_price is None:
        raise ValueError(
            "market pricing must define fixed_quote_per_base or "
            "min/max_price_quote_per_base for offer build"
        )
    return float(quote_price)


def log_signed_offer_artifact(
    *,
    offer_text: str,
    ticker: str,
    amount: int,
    trading_pair: str,
    expiry: str,
) -> None:
    coin_id_hints = _extract_coin_id_hints_from_offer_text(offer_text)
    coin_id = coin_id_hints[0] if coin_id_hints else ""
    _runtime_logger.debug("signed_offer_file:%s", offer_text)
    _runtime_logger.info(
        "signed_offer_metadata:ticker=%s coinid=%s amount=%s trading_pair=%s expiry=%s",
        ticker,
        coin_id,
        amount,
        trading_pair,
        expiry,
    )


def _condition_has_offer_expiration(condition: object) -> bool:
    parse_names = (
        "parse_assert_before_seconds_relative",
        "parse_assert_before_seconds_absolute",
        "parse_assert_before_height_relative",
        "parse_assert_before_height_absolute",
    )
    for parse_name in parse_names:
        parse_fn = getattr(condition, parse_name, None)
        if not callable(parse_fn):
            continue
        try:
            if parse_fn() is not None:
                return True
        except Exception:
            continue
    return False


def _extract_offer_conditions_from_coin_spend(sdk: object, coin_spend: object) -> list[object]:
    clvm_cls = getattr(sdk, "Clvm", None)
    if not callable(clvm_cls):
        return []
    puzzle_reveal = getattr(coin_spend, "puzzle_reveal", None)
    solution = getattr(coin_spend, "solution", None)
    if not isinstance(puzzle_reveal, bytes | bytearray | memoryview) or not isinstance(
        solution, bytes | bytearray | memoryview
    ):
        return []
    try:
        clvm = clvm_cls()
        deserialize_fn = getattr(clvm, "deserialize", None)
        if not callable(deserialize_fn):
            return []
        puzzle_program = deserialize_fn(bytes(puzzle_reveal))
        solution_program = deserialize_fn(bytes(solution))
        run_fn = getattr(puzzle_program, "run", None)
        if not callable(run_fn):
            return []
        run_output = run_fn(solution_program, 1_000_000_000_000, True)
        value = getattr(run_output, "value", None)
        if value is None:
            return []
        to_list_fn = getattr(value, "to_list", None)
        if callable(to_list_fn):
            parsed = to_list_fn() or []
            if isinstance(parsed, collections.abc.Iterable) and not isinstance(
                parsed, bytes | bytearray | str
            ):
                return list(parsed)
        if isinstance(value, collections.abc.Iterable) and not isinstance(
            value, bytes | bytearray | str
        ):
            return list(value)
    except Exception:
        return []
    return []


def _offer_has_expiration_condition(sdk: object, offer_text: str) -> bool:
    decode_offer = getattr(sdk, "decode_offer", None)
    if not callable(decode_offer):
        return False
    spend_bundle = decode_offer(offer_text)
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    for coin_spend in coin_spends:
        for condition in _extract_offer_conditions_from_coin_spend(sdk, coin_spend):
            if _condition_has_offer_expiration(condition):
                return True
    return False


def _offer_has_duplicate_spent_coin_ids(sdk: object, offer_text: str) -> bool:
    decode_offer = getattr(sdk, "decode_offer", None)
    to_hex = getattr(sdk, "to_hex", None)
    if not callable(decode_offer) or not callable(to_hex):
        return False
    try:
        spend_bundle = decode_offer(offer_text)
    except Exception:
        return False
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    seen: set[str] = set()
    for coin_spend in coin_spends:
        coin = getattr(coin_spend, "coin", None)
        if coin is None:
            continue
        coin_id_fn = getattr(coin, "coin_id", None)
        if not callable(coin_id_fn):
            continue
        try:
            coin_id_hex = str(to_hex(coin_id_fn())).strip().lower()
        except Exception:
            continue
        normalized = normalize_hex_id(coin_id_hex)
        if not normalized:
            continue
        if normalized in seen:
            return True
        seen.add(normalized)
    return False


def verify_offer_text_for_dexie(offer_text: str) -> str | None:
    native_validated = False
    try:
        native = importlib.import_module("greenfloor_native")
    except Exception:
        native = None
    else:
        try:
            native.validate_offer(offer_text)
            native_validated = True
        except Exception as exc:
            return f"wallet_sdk_offer_validate_failed:{exc}"
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception as exc:
        if native_validated:
            return None
        return f"wallet_sdk_import_error:{exc}"
    try:
        decode_offer = getattr(sdk, "decode_offer", None)
        decode_available = callable(decode_offer)
        if not native_validated:
            validate_offer = getattr(sdk, "validate_offer", None)
            if callable(validate_offer):
                validate_offer(offer_text)
            else:
                verify_offer = getattr(sdk, "verify_offer", None)
                if not callable(verify_offer):
                    return "wallet_sdk_validate_offer_unavailable"
                if not bool(verify_offer(offer_text)):
                    return "wallet_sdk_offer_verify_false"
        if native_validated and not decode_available:
            return None
        if _offer_has_duplicate_spent_coin_ids(sdk, offer_text):
            return "wallet_sdk_offer_duplicate_spent_coin_ids"
        if not _offer_has_expiration_condition(sdk, offer_text):
            return "wallet_sdk_offer_missing_expiration"
    except Exception as exc:
        return f"wallet_sdk_offer_validate_failed:{exc}"
    return None


def post_dexie_offer_with_invalid_offer_retry(
    *,
    dexie: DexieAdapter,
    offer_text: str,
    drop_only: bool,
    claim_rewards: bool,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> dict[str, Any]:
    if sleep_fn is None:
        sleep_fn = time.sleep
    attempt = 0
    sleep_seconds = _DEXIE_INVALID_OFFER_RETRY_INITIAL_DELAY_SECONDS
    while True:
        result = dexie.post_offer(
            offer_text,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
        )
        error = str(result.get("error", "")).strip()
        should_retry = (
            bool(error)
            and "dexie_http_error:400" in error
            and "Invalid Offer" in error
            and attempt < (_DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS - 1)
        )
        if not should_retry:
            return result
        attempt += 1
        sleep_fn(sleep_seconds)
        sleep_seconds = min(8.0, sleep_seconds * 2.0)


def verify_dexie_offer_visible_by_id(
    *,
    dexie: DexieAdapter,
    offer_id: str,
    max_attempts: int = 4,
    delay_seconds: float = 1.5,
    expected_offered_asset_id: str | None = None,
    expected_offered_symbol: str | None = None,
    expected_requested_asset_id: str | None = None,
    expected_requested_symbol: str | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> str | None:
    if sleep_fn is None:
        sleep_fn = time.sleep
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return "dexie_offer_missing_id_after_publish"
    attempts = max(1, int(max_attempts))
    last_error = "dexie_offer_not_visible_after_publish"
    for attempt in range(1, attempts + 1):
        try:
            payload = dexie.get_offer(clean_offer_id)
        except Exception as exc:
            last_error = f"dexie_get_offer_error:{exc}"
            if attempt < attempts:
                sleep_fn(delay_seconds)
            continue
        offer_payload = payload.get("offer") if isinstance(payload, dict) else None
        visible_id = (
            str(offer_payload.get("id", "")).strip() if isinstance(offer_payload, dict) else ""
        )
        if visible_id == clean_offer_id:
            if isinstance(offer_payload, dict):
                offered = offer_payload.get("offered")
                requested = offer_payload.get("requested")
                if expected_offered_asset_id and isinstance(offered, list):
                    expected_asset = str(expected_offered_asset_id).strip().lower()
                    expected_symbol = str(expected_offered_symbol or "").strip().lower()
                    found = any(
                        isinstance(row, dict)
                        and (
                            str(row.get("id", "")).strip().lower() == expected_asset
                            or (
                                expected_symbol
                                and str(row.get("code", "")).strip().lower() == expected_symbol
                            )
                            or (
                                expected_symbol
                                and str(row.get("name", "")).strip().lower() == expected_symbol
                            )
                        )
                        for row in offered
                    )
                    if not found:
                        return (
                            "dexie_offer_offered_asset_missing:"
                            f"expected_asset={expected_offered_asset_id}:"
                            f"expected_symbol={expected_offered_symbol}"
                        )
                if expected_requested_asset_id and isinstance(requested, list):
                    expected_asset = str(expected_requested_asset_id).strip().lower()
                    expected_symbol = str(expected_requested_symbol or "").strip().lower()
                    found = any(
                        isinstance(row, dict)
                        and (
                            str(row.get("id", "")).strip().lower() == expected_asset
                            or (
                                expected_symbol
                                and str(row.get("code", "")).strip().lower() == expected_symbol
                            )
                            or (
                                expected_symbol
                                and str(row.get("name", "")).strip().lower() == expected_symbol
                            )
                        )
                        for row in requested
                    )
                    if not found:
                        return (
                            "dexie_offer_requested_asset_missing:"
                            f"expected_asset={expected_requested_asset_id}:"
                            f"expected_symbol={expected_requested_symbol}"
                        )
            return None
        last_error = "dexie_offer_visibility_payload_mismatch"
        if attempt < attempts:
            sleep_fn(delay_seconds)
    return last_error


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    normalized = str(error).strip().lower()
    return (
        "dexie_get_offer_error" in normalized and "404" in normalized
    ) or "dexie_http_error:404" in normalized


def verify_offer_visible_on_dexie(
    *,
    dexie: DexieAdapter,
    offer_id: str,
    max_attempts: int = 4,
    delay_seconds: float = 1.5,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> tuple[bool, str]:
    """Return *(visible, error)* after polling Dexie for a freshly posted offer id."""
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return False, "missing_offer_id"
    visibility_error = verify_dexie_offer_visible_by_id(
        dexie=dexie,
        offer_id=clean_offer_id,
        max_attempts=max_attempts,
        delay_seconds=delay_seconds,
        sleep_fn=sleep_fn,
    )
    if visibility_error is None:
        return True, ""
    return False, visibility_error


def post_offer_phase(
    *,
    publish_venue: str,
    dexie: DexieAdapter | None,
    splash: SplashAdapter | None,
    offer_text: str,
    drop_only: bool,
    claim_rewards: bool,
    market: Any,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
    post_dexie_offer_with_invalid_offer_retry_fn: collections.abc.Callable[..., dict[str, Any]]
    | None = None,
    verify_dexie_offer_visible_by_id_fn: collections.abc.Callable[..., str | None] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> dict[str, Any]:
    _ = market
    if post_dexie_offer_with_invalid_offer_retry_fn is None:
        post_dexie_offer_with_invalid_offer_retry_fn = post_dexie_offer_with_invalid_offer_retry
    if verify_dexie_offer_visible_by_id_fn is None:
        verify_dexie_offer_visible_by_id_fn = verify_dexie_offer_visible_by_id
    if sleep_fn is None:
        sleep_fn = time.sleep
    if publish_venue == "dexie":
        assert dexie is not None
        last_result: dict[str, Any] = {}
        last_visibility_error = ""
        for attempt in range(1, _DEXIE_VISIBILITY_POST_MAX_ATTEMPTS + 1):
            result = post_dexie_offer_with_invalid_offer_retry_fn(
                dexie=dexie,
                offer_text=offer_text,
                drop_only=drop_only,
                claim_rewards=claim_rewards,
            )
            last_result = dict(result)
            if not bool(result.get("success", False)):
                return result
            posted_offer_id = str(result.get("id", "")).strip()
            visibility_error = verify_dexie_offer_visible_by_id_fn(
                dexie=dexie,
                offer_id=posted_offer_id,
                expected_offered_asset_id=str(expected_offered_asset_id),
                expected_offered_symbol=str(expected_offered_symbol),
                expected_requested_asset_id=str(expected_requested_asset_id),
                expected_requested_symbol=str(expected_requested_symbol),
            )
            if not visibility_error:
                return result
            last_visibility_error = str(visibility_error)
            if not is_transient_dexie_visibility_404_error(last_visibility_error):
                return {
                    **result,
                    "success": False,
                    "error": last_visibility_error,
                }
            if attempt < _DEXIE_VISIBILITY_POST_MAX_ATTEMPTS:
                sleep_fn(_DEXIE_VISIBILITY_POST_DELAY_SECONDS)
        return {
            **last_result,
            "success": False,
            "error": (last_visibility_error or "dexie_offer_not_visible_after_publish"),
        }
    assert splash is not None
    return splash.post_offer(offer_text)


def expected_publish_asset_fields(
    *,
    side: str,
    market: MarketConfig,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> dict[str, str]:
    is_buy = normalize_offer_side(side) == "buy"
    if is_buy:
        return {
            "expected_offered_asset_id": str(resolved_quote_asset_id),
            "expected_offered_symbol": str(market.quote_asset),
            "expected_requested_asset_id": str(resolved_base_asset_id),
            "expected_requested_symbol": str(market.base_symbol),
        }
    return {
        "expected_offered_asset_id": str(resolved_base_asset_id),
        "expected_offered_symbol": str(market.base_symbol),
        "expected_requested_asset_id": str(resolved_quote_asset_id),
        "expected_requested_symbol": str(market.quote_asset),
    }
