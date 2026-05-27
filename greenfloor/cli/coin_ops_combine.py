"""CLI coin-combine command."""

from __future__ import annotations

from pathlib import Path

from greenfloor.cli.coin_ops_cli import execute_combine_cli


def coin_combine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    number_of_coins: int,
    asset_id: str | None,
    coin_ids: list[str],
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")

    return execute_combine_cli(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        coin_ids=coin_ids,
        number_of_coins=number_of_coins,
        asset_id=asset_id,
        no_wait=no_wait,
        size_base_units=size_base_units,
        until_ready=until_ready,
        max_iterations=max_iterations,
    )
