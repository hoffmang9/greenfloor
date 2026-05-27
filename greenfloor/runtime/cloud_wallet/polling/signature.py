from __future__ import annotations

import collections.abc
import sys
import time
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.moderate_retry import call_with_moderate_retry


def poll_signature_request_until_not_unsigned(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    timeout_seconds: int,
    warning_interval_seconds: int,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> tuple[str, list[dict[str, str]]]:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic
    events: list[dict[str, str]] = []
    start = monotonic_fn()
    next_warning = warning_interval_seconds
    warning_count = 0
    next_heartbeat = 5
    sleep_seconds = 2.0
    while True:
        elapsed = int(monotonic_fn() - start)
        status_payload = retry_fn(
            action="wallet_get_signature_request",
            call=lambda: wallet.get_signature_request(signature_request_id=signature_request_id),
            elapsed_seconds=elapsed,
            events=events,
        )
        status = str(status_payload.get("status", "")).strip().upper()
        if status and status != "UNSIGNED":
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            print(
                f"signature submitted: {signature_request_id} status={status}",
                file=sys.stderr,
                flush=True,
            )
            return status, events
        if elapsed >= next_heartbeat:
            print(".", end="", file=sys.stderr, flush=True)
            next_heartbeat += 5
        if elapsed >= timeout_seconds:
            raise RuntimeError("signature_request_timeout_waiting_for_signature")
        if elapsed >= next_warning:
            warning_count += 1
            events.append(
                {
                    "event": "signature_wait_warning",
                    "elapsed_seconds": str(elapsed),
                    "signing_state_age_seconds": str(elapsed),
                    "message": "still_waiting_on_user_signature",
                    "wait_reason": "waiting_on_user_signature",
                    "warning_count": str(warning_count),
                }
            )
            if warning_count >= 2:
                events.append(
                    {
                        "event": "signature_wait_escalation",
                        "elapsed_seconds": str(elapsed),
                        "message": "extended_user_signature_delay",
                        "wait_reason": "waiting_on_user_signature",
                        "warning_count": str(warning_count),
                    }
                )
            next_warning += warning_interval_seconds
        sleep_fn(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)
