"""CLI coin-split command."""

from __future__ import annotations

from pathlib import Path

from greenfloor.cli.coin_ops_cli import execute_split_cli


def coin_split(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    coin_ids: list[str],
    amount_per_coin: int,
    number_of_coins: int,
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
    allow_lock_all_spendable: bool = False,
    force_split_when_ready: bool = False,
    prompt_for_override: bool | None = None,
) -> int:
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and size_base_units is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")

    return execute_split_cli(
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        venue=venue,
        coin_ids=coin_ids,
        amount_per_coin=amount_per_coin,
        number_of_coins=number_of_coins,
        no_wait=no_wait,
        size_base_units=size_base_units,
        until_ready=until_ready,
        max_iterations=max_iterations,
        allow_lock_all_spendable=allow_lock_all_spendable,
        force_split_when_ready=force_split_when_ready,
        prompt_for_override=prompt_for_override,
    )
