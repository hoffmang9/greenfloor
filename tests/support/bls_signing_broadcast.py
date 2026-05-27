"""Test-only broadcast helpers for BLS signing (structured Coinset push_tx fallback)."""

from __future__ import annotations

from typing import Any

from greenfloor.runtime.coinset_runtime import _coinset_adapter


def _broadcast_spend_bundle(*, sdk: Any, spend_bundle_hex: str, network: str) -> dict[str, Any]:
    try:
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        spend_bundle_bytes = bytes.fromhex(raw_hex)
    except ValueError:
        return {
            "status": "skipped",
            "reason": "invalid_spend_bundle_hex",
            "operation_id": None,
        }

    try:
        spend_bundle = sdk.SpendBundle.from_bytes(spend_bundle_bytes)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"spend_bundle_decode_error:{exc}",
            "operation_id": None,
        }

    coinset = _coinset_adapter(network=network)
    try:
        response = coinset.push_tx(spend_bundle_hex=spend_bundle_hex)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"push_tx_error:{exc}",
            "operation_id": None,
        }
    if not bool(response.get("success", False)):
        error_text = str(response.get("error") or "").strip().lower()
        if "expected struct spendbundle" in error_text or "invalid type: string" in error_text:
            try:
                structured_bundle = _spend_bundle_to_coinset_json(
                    sdk=sdk, spend_bundle=spend_bundle
                )
                response = coinset.push_tx_structured(spend_bundle=structured_bundle)
            except Exception as exc:
                return {
                    "status": "skipped",
                    "reason": f"push_tx_structured_error:{exc}",
                    "operation_id": None,
                }
    if not bool(response.get("success", False)):
        err = response.get("error") or "push_tx_rejected"
        return {"status": "skipped", "reason": str(err), "operation_id": None}
    tx_id = sdk.to_hex(spend_bundle.hash())
    return {
        "status": "executed",
        "reason": str(response.get("status", "submitted")),
        "operation_id": tx_id,
    }


def _as_0x_hex(value: Any) -> str:
    if isinstance(value, bytes | bytearray | memoryview):
        return f"0x{bytes(value).hex()}"
    as_bytes = bytes(value)
    return f"0x{as_bytes.hex()}"


def _spend_bundle_to_coinset_json(*, sdk: Any, spend_bundle: Any) -> dict[str, Any]:
    coin_spends_payload: list[dict[str, Any]] = []
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    for coin_spend in coin_spends:
        coin = getattr(coin_spend, "coin", None)
        if coin is None:
            continue
        coin_spends_payload.append(
            {
                "coin": {
                    "parent_coin_info": _as_0x_hex(coin.parent_coin_info),
                    "puzzle_hash": _as_0x_hex(coin.puzzle_hash),
                    "amount": int(coin.amount),
                },
                "puzzle_reveal": _as_0x_hex(coin_spend.puzzle_reveal),
                "solution": _as_0x_hex(coin_spend.solution),
            }
        )
    aggregated_signature = _as_0x_hex(spend_bundle.aggregated_signature.to_bytes())
    return {
        "coin_spends": coin_spends_payload,
        "aggregated_signature": aggregated_signature,
    }
