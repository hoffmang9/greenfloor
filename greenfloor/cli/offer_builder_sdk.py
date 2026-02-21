from __future__ import annotations

import hashlib
import json
import sys
from typing import Any


def _import_sdk() -> Any:
    import chia_wallet_sdk as sdk  # type: ignore

    return sdk


def _build_offer(payload: dict[str, Any], sdk: Any) -> str:
    receive_address = str(payload.get("receive_address", "")).strip()
    if not receive_address:
        raise ValueError("missing_receive_address")

    size_base_units = int(payload.get("size_base_units", 0))
    if size_base_units <= 0:
        raise ValueError("invalid_size_base_units")

    address = sdk.Address.decode(receive_address)
    puzzle_hash = address.puzzle_hash
    seed = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    parent_coin_info = hashlib.sha256(seed).digest()
    coin = sdk.Coin(parent_coin_info, puzzle_hash, size_base_units)
    # minimal nil puzzle/solution programs represented as serialized bytes.
    coin_spend = sdk.CoinSpend(coin, b"\x80", b"\x80")
    spend_bundle = sdk.SpendBundle([coin_spend], sdk.Signature.infinity())
    return str(sdk.encode_offer(spend_bundle))


def main() -> None:
    raw = sys.stdin.read()
    try:
        payload = json.loads(raw or "{}")
    except json.JSONDecodeError:
        print(json.dumps({"status": "skipped", "reason": "invalid_request_json"}))
        raise SystemExit(0) from None
    if not isinstance(payload, dict):
        print(json.dumps({"status": "skipped", "reason": "invalid_request_payload"}))
        raise SystemExit(0)

    try:
        sdk = _import_sdk()
    except Exception as exc:
        print(json.dumps({"status": "skipped", "reason": f"wallet_sdk_import_error:{exc}"}))
        raise SystemExit(0) from None

    try:
        offer = _build_offer(payload, sdk)
    except Exception as exc:
        print(json.dumps({"status": "skipped", "reason": f"wallet_sdk_offer_build_failed:{exc}"}))
        raise SystemExit(0) from None

    print(
        json.dumps(
            {"status": "executed", "reason": "wallet_sdk_offer_build_success", "offer": offer}
        )
    )


if __name__ == "__main__":
    main()
