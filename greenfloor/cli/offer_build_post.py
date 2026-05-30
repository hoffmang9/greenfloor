"""CLI build-and-post offer commands."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_program_config
from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from greenfloor.runtime.json_output import format_json_output
from greenfloor.runtime.resolved_daemon_paths import ResolvedDaemonPaths


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

    program = load_program_config(program_path)
    paths = ResolvedDaemonPaths(
        program_path=program_path.expanduser().resolve(),
        markets_path=markets_path.expanduser().resolve(),
        testnet_markets_path=(
            testnet_markets_path.expanduser().resolve()
            if testnet_markets_path is not None
            else None
        ),
    )
    exit_code, payload = run_build_and_post_offer_in_process(
        paths=paths,
        network=network,
        market_id=market_id,
        pair=pair,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue or program.offer_publish_venue,
        dexie_base_url=dexie_base_url or program.dexie_api_base,
        splash_base_url=splash_base_url or program.splash_api_base,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        dry_run=dry_run,
        persist_results=True,
    )
    print(format_json_output(payload))
    return exit_code
