"""In-process Rust engine build-and-post orchestration (daemon + managed paths)."""

from __future__ import annotations

from pathlib import Path
from typing import Any, cast

from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.runtime.daemon_config_paths import DaemonConfigPaths


def _engine_build_and_post_offer() -> Any:
    return require_engine_method(
        import_engine(),
        "build_and_post_offer",
        missing="build_and_post_offer",
    )


def run_build_and_post_offer_in_process(
    *,
    paths: DaemonConfigPaths,
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

    request: dict[str, Any] = {
        "program_path": str(paths.program_path),
        "markets_path": str(paths.markets_path),
        "network": network.strip(),
        "size_base_units": int(size_base_units),
        "repeat": int(repeat),
        "drop_only": bool(drop_only),
        "claim_rewards": bool(claim_rewards),
        "dry_run": bool(dry_run),
        "persist_results": bool(persist_results),
    }
    if paths.testnet_markets_path is not None:
        request["testnet_markets_path"] = str(paths.testnet_markets_path)
    if market_id:
        request["market_id"] = market_id.strip()
    if pair:
        request["pair"] = pair.strip()
    if publish_venue and publish_venue.strip():
        request["publish_venue"] = publish_venue.strip()
    if dexie_base_url and dexie_base_url.strip():
        request["dexie_base_url"] = dexie_base_url.strip()
    if splash_base_url and splash_base_url.strip():
        request["splash_base_url"] = splash_base_url.strip()
    if action_side and action_side.strip():
        request["action_side"] = action_side.strip()

    response = _engine_build_and_post_offer()(request)
    if not isinstance(response, dict):
        raise TypeError("engine build_and_post_offer returned non-dict response")
    exit_code = int(response.get("exit_code", 1))
    payload = response.get("payload", {})
    if not isinstance(payload, dict):
        raise TypeError("engine build_and_post_offer payload is not a dict")
    return exit_code, cast(dict[str, Any], payload)
