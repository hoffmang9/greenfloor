"""Rust-backed offer-build and retry policy (single canonical bridge)."""

from __future__ import annotations

from typing import Any, TypedDict

from greenfloor.core import kernel_bridge

_KERNEL_REBUILD_HINT = (
    "greenfloor_signer extension is missing required policy symbols. "
    "Rebuild it (for example: `maturin develop --manifest-path "
    "greenfloor-signer-pyo3/Cargo.toml`)."
)


def _require_policy_method(method_name: str):
    kernel = kernel_bridge.policy_kernel()
    method = getattr(kernel, method_name, None)
    if method is None:
        raise RuntimeError(f"{_KERNEL_REBUILD_HINT} Missing symbol: {method_name}")
    return method


class ExpectedPublishAssetFields(TypedDict):
    expected_offered_asset_id: str
    expected_offered_symbol: str
    expected_requested_asset_id: str
    expected_requested_symbol: str


def _coerce_expected_publish_asset_fields(payload: object) -> ExpectedPublishAssetFields:
    if not isinstance(payload, dict):
        raise TypeError("expected_publish_asset_fields must return dict payload")
    required_keys = (
        "expected_offered_asset_id",
        "expected_offered_symbol",
        "expected_requested_asset_id",
        "expected_requested_symbol",
    )
    missing = [key for key in required_keys if key not in payload]
    if missing:
        raise TypeError("expected_publish_asset_fields missing keys: " + ", ".join(sorted(missing)))
    return {
        "expected_offered_asset_id": str(payload["expected_offered_asset_id"]),
        "expected_offered_symbol": str(payload["expected_offered_symbol"]),
        "expected_requested_asset_id": str(payload["expected_requested_asset_id"]),
        "expected_requested_symbol": str(payload["expected_requested_symbol"]),
    }


def resolve_offer_expiry_for_pricing(pricing: dict[str, Any]) -> tuple[str, int]:
    unit, value = kernel_bridge.policy_kernel().resolve_offer_expiry_for_pricing(pricing)
    return str(unit), int(value)


def resolve_quote_price_for_pricing(pricing: dict[str, Any]) -> float:
    return float(kernel_bridge.policy_kernel().resolve_quote_price_for_pricing(pricing))


def mojo_multiplier_for_leg(pricing: dict[str, Any], field: str, asset_id: str) -> int:
    return int(kernel_bridge.policy_kernel().mojo_multiplier_for_leg(pricing, field, asset_id))


def _coerce_signer_offer_leg_amounts(payload: object):
    from greenfloor.core.signer_offer_request import SignerOfferLegAmounts

    if isinstance(payload, SignerOfferLegAmounts):
        return payload
    raise TypeError("compute_signer_offer_leg_amounts must return SignerOfferLegAmounts")


def normalize_offer_side(action_side: str) -> str:
    """Normalize offer action side to ``buy`` or ``sell`` (pure Python; matches Rust kernel)."""
    value = str(action_side or "").strip()
    if value.lower() == "buy":
        return "buy"
    return "sell"


def quote_mojos_for_base_size(
    *,
    size_base_units: int,
    quote_price: float,
    quote_unit_multiplier: int,
) -> int:
    compute = _require_policy_method("quote_mojos_for_base_size")
    return int(
        compute(
            int(size_base_units),
            float(quote_price),
            int(quote_unit_multiplier),
        )
    )


def signer_split_asset_id(
    *,
    action_side: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> str:
    resolve = _require_policy_method("signer_split_asset_id")
    return str(
        resolve(
            str(action_side),
            str(resolved_base_asset_id),
            str(resolved_quote_asset_id),
        )
    )


def normalize_offer_asset_id(asset_id: str) -> str:
    return str(_require_policy_method("normalize_offer_asset_id")(str(asset_id)))


def compute_signer_offer_leg_amounts(
    *,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str,
    pricing: dict[str, Any],
) -> SignerOfferLegAmounts:
    compute = _require_policy_method("compute_signer_offer_leg_amounts")
    payload = compute(
        int(size_base_units),
        float(quote_price),
        str(resolved_base_asset_id),
        str(resolved_quote_asset_id),
        str(action_side),
        dict(pricing),
    )
    return _coerce_signer_offer_leg_amounts(payload)


def verify_offer_for_dexie(offer_text: str) -> str | None:
    try:
        verify_offer = _require_policy_method("verify_offer_for_dexie")
        error = verify_offer(offer_text)
    except ImportError:
        return "wallet_sdk_import_error:greenfloor_signer_unavailable"
    return None if error is None else str(error)


def bootstrap_block_error(
    *,
    bootstrap_status: str,
    bootstrap_reason: str,
    bootstrap_ready: bool,
) -> str | None:
    compute_error = _require_policy_method("bootstrap_block_error")
    error = compute_error(
        str(bootstrap_status),
        str(bootstrap_reason),
        bool(bootstrap_ready),
    )
    return None if error is None else str(error)


def expected_publish_asset_fields(
    *,
    side: str,
    base_symbol: str,
    quote_asset: str,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
) -> ExpectedPublishAssetFields:
    resolve_fields = _require_policy_method("expected_publish_asset_fields")
    payload = resolve_fields(
        str(side),
        str(base_symbol),
        str(quote_asset),
        str(resolved_base_asset_id),
        str(resolved_quote_asset_id),
    )
    return _coerce_expected_publish_asset_fields(payload)


def dexie_offer_asset_expectation_error(
    *,
    offered: object,
    requested: object,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
) -> str | None:
    verify_assets = _require_policy_method("dexie_offer_asset_expectation_error")
    error = verify_assets(
        offered,
        requested,
        str(expected_offered_asset_id),
        str(expected_offered_symbol),
        str(expected_requested_asset_id),
        str(expected_requested_symbol),
    )
    return None if error is None else str(error)


def parse_rate_limit_retry_seconds(error_text: str) -> float | None:
    value = kernel_bridge.policy_kernel().parse_rate_limit_retry_seconds(error_text)
    return None if value is None else float(value)


def moderate_retry_sleep_seconds(
    *,
    current_sleep: float,
    rate_limit_wait: float | None,
) -> float:
    return float(
        kernel_bridge.policy_kernel().moderate_retry_sleep_seconds(
            float(current_sleep), rate_limit_wait
        )
    )


def moderate_retry_next_sleep(current_sleep: float) -> float:
    return float(kernel_bridge.policy_kernel().moderate_retry_next_sleep(float(current_sleep)))


def dexie_invalid_offer_should_retry(*, error: str, attempt: int, max_attempts: int) -> bool:
    return bool(
        kernel_bridge.policy_kernel().dexie_invalid_offer_should_retry(
            str(error),
            int(attempt),
            int(max_attempts),
        )
    )


def dexie_invalid_offer_retry_sleep(*, attempt: int, initial_sleep: float) -> float:
    return float(
        kernel_bridge.policy_kernel().dexie_invalid_offer_retry_sleep(
            int(attempt), float(initial_sleep)
        )
    )


def coinset_fee_lookup_retry_sleep(attempt: int) -> float:
    return float(kernel_bridge.policy_kernel().coinset_fee_lookup_retry_sleep(int(attempt)))


def poll_exponential_sleep_now(
    *,
    elapsed_seconds: int,
    timeout_seconds: int,
    sleep_seconds: float,
    initial_sleep: float,
    max_sleep: float,
) -> float | None:
    value = kernel_bridge.policy_kernel().poll_exponential_sleep_now(
        int(elapsed_seconds),
        int(timeout_seconds),
        float(sleep_seconds),
        float(initial_sleep),
        float(max_sleep),
    )
    return None if value is None else float(value)


def poll_exponential_advance_sleep(
    *,
    sleep_seconds: float,
    initial_sleep: float,
    max_sleep: float,
    multiplier: float,
) -> float:
    return float(
        kernel_bridge.policy_kernel().poll_exponential_advance_sleep(
            float(sleep_seconds),
            float(initial_sleep),
            float(max_sleep),
            float(multiplier),
        )
    )
