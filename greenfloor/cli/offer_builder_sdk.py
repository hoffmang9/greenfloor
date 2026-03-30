from __future__ import annotations

import json
import sys

from greenfloor.offer_builder import build_offer


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
        offer = build_offer(payload)
    except Exception as exc:
        print(
            json.dumps(
                {
                    "status": "skipped",
                    "reason": f"offer_builder_failed:{exc}",
                }
            )
        )
        raise SystemExit(0) from None

    print(
        json.dumps(
            {
                "status": "executed",
                "reason": "wallet_sdk_offer_build_success",
                "offer": offer,
            }
        )
    )


if __name__ == "__main__":
    main()
