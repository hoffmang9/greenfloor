"""CLI build-and-post offer commands."""

from __future__ import annotations

from pathlib import Path

from greenfloor.cli.engine_binary import run_build_and_post_offer_via_engine
from greenfloor.runtime.json_output import json_output_compact


def build_and_post_offer_cli(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    size_base_units: int,
    repeat: int,
    publish_venue: str | None,
    dexie_base_url: str | None,
    splash_base_url: str | None,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
) -> int:
    if size_base_units <= 0:
        raise ValueError("size_base_units must be positive")
    if repeat <= 0:
        raise ValueError("repeat must be positive")

    return run_build_and_post_offer_via_engine(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        dry_run=dry_run,
        compact_json=json_output_compact(),
    )
