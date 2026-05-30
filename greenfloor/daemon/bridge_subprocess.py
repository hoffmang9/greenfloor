"""JSON stdin/stdout bridge for native greenfloor-engine daemon IO phases."""

from __future__ import annotations

import json
import sys
import traceback
from typing import Any, Callable

from greenfloor.daemon.rust_cycle_bridge import (
    run_cycle_preamble,
    run_market_coin_ops_phase,
    run_market_cycle_io_phases,
)

BridgeFn = Callable[..., dict[str, Any]]

_METHODS: dict[str, BridgeFn] = {
    "run_cycle_preamble": run_cycle_preamble,
    "run_market_cycle_io_phases": run_market_cycle_io_phases,
    "run_market_coin_ops_phase": run_market_coin_ops_phase,
}


def _dispatch(request: dict[str, Any]) -> dict[str, Any]:
    method = str(request.get("method", "")).strip()
    fn = _METHODS.get(method)
    if fn is None:
        raise ValueError(f"unknown bridge method: {method}")
    kwargs = request.get("kwargs", {})
    if not isinstance(kwargs, dict):
        raise TypeError("kwargs must be a dict")
    return fn(**kwargs)


def main() -> None:
    try:
        raw = sys.stdin.read()
        request = json.loads(raw) if raw.strip() else {}
        if not isinstance(request, dict):
            raise TypeError("bridge request must be a JSON object")
        result = _dispatch(request)
        json.dump({"ok": True, "result": result}, sys.stdout)
    except Exception as exc:  # pragma: no cover - surfaced to Rust caller
        json.dump(
            {
                "ok": False,
                "error": str(exc),
                "traceback": traceback.format_exc(),
            },
            sys.stdout,
        )
        raise SystemExit(1) from exc


if __name__ == "__main__":
    main()
