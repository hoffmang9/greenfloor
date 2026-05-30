"""In-process Rust engine build-and-post orchestration (daemon + managed paths)."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from greenfloor.core.engine_bridge import import_engine, require_engine_method

__all__ = ["run_build_and_post_offer_in_process"]


def run_build_and_post_offer_in_process(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None = None,
    pair: str | None = None,
    size_base_units: int,
    repeat: int = 1,
    publish_venue: str | None = None,
    dexie_base_url: str | None = None,
    splash_base_url: str | None = None,
    drop_only: bool = True,
    claim_rewards: bool = False,
    dry_run: bool = False,
    action_side: str | None = None,
    persist_results: bool = False,
) -> tuple[int, dict[str, Any]]:
    if size_base_units <= 0:
        raise ValueError("size_base_units must be positive")
    if repeat <= 0:
        raise ValueError("repeat must be positive")
    if market_id is None and pair is None:
        raise ValueError("provide exactly one of market_id or pair")
    if market_id is not None and pair is not None:
        raise ValueError("provide exactly one of market_id or pair")

    engine = import_engine()
    run_fn = require_engine_method(
        engine,
        "build_and_post_offer",
        missing="build_and_post_offer",
    )
    request = {
        "program_path": str(program_path),
        "markets_path": str(markets_path),
        "testnet_markets_path": (
            str(testnet_markets_path) if testnet_markets_path is not None else None
        ),
        "network": network.strip(),
        "market_id": market_id.strip() if market_id else None,
        "pair": pair.strip() if pair else None,
        "size_base_units": int(size_base_units),
        "repeat": int(repeat),
        "publish_venue": publish_venue.strip() if publish_venue and publish_venue.strip() else None,
        "dexie_base_url": dexie_base_url.strip()
        if dexie_base_url and dexie_base_url.strip()
        else None,
        "splash_base_url": splash_base_url.strip()
        if splash_base_url and splash_base_url.strip()
        else None,
        "drop_only": bool(drop_only),
        "claim_rewards": bool(claim_rewards),
        "dry_run": bool(dry_run),
        "compact_json": False,
        "persist_results": bool(persist_results),
        "action_side": action_side.strip() if action_side and action_side.strip() else None,
    }
    response = run_fn(request)
    payload = response["payload"]
    if not isinstance(payload, dict):
        raise TypeError("engine build_and_post_offer payload is not a dict")
    return int(response["exit_code"]), dict(payload)
