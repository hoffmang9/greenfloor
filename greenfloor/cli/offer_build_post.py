"""CLI build-and-post offer commands."""

from __future__ import annotations

import logging
from pathlib import Path

from greenfloor.cli.engine_binary import run_build_and_post_offer_via_engine
from greenfloor.config.io import (
    is_testnet,
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
)
from greenfloor.config.models import require_signer_offer_path
from greenfloor.logging_setup import warn_if_log_level_auto_healed
from greenfloor.runtime.json_output import json_output_compact
from greenfloor.runtime.offer_publish import initialize_manager_file_logging

_manager_logger = logging.getLogger("greenfloor.manager")


def resolve_dexie_base_url(network: str, explicit_base_url: str | None) -> str:
    if explicit_base_url and explicit_base_url.strip():
        return explicit_base_url.strip().rstrip("/")
    network_l = network.strip().lower()
    if network_l in {"mainnet", ""}:
        return "https://api.dexie.space"
    if is_testnet(network_l):
        return "https://api-testnet.dexie.space"
    raise ValueError(f"unsupported network for dexie posting: {network}")


def resolve_splash_base_url(explicit_base_url: str | None) -> str:
    if explicit_base_url and explicit_base_url.strip():
        return explicit_base_url.strip().rstrip("/")
    return "http://john-deere.hoffmang.com:4000"


def resolve_offer_publish_settings(
    *,
    program_path: Path,
    network: str,
    venue_override: str | None,
    dexie_base_url: str | None,
    splash_base_url: str | None,
) -> tuple[str, str, str]:
    program = load_program_config(program_path)
    venue = (venue_override or program.offer_publish_venue).strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("offer publish venue must be dexie or splash")
    if dexie_base_url and dexie_base_url.strip():
        dexie_base = dexie_base_url.strip().rstrip("/")
    elif is_testnet(network):
        dexie_base = resolve_dexie_base_url(network, None)
    else:
        dexie_base = str(program.dexie_api_base).strip().rstrip("/")
    if splash_base_url and splash_base_url.strip():
        splash_base = splash_base_url.strip().rstrip("/")
    else:
        splash_base = str(program.splash_api_base).strip().rstrip("/")
    return venue, dexie_base, splash_base


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
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
) -> int:
    if size_base_units <= 0:
        raise ValueError("size_base_units must be positive")
    if repeat <= 0:
        raise ValueError("repeat must be positive")

    program = load_program_config(program_path)
    require_signer_offer_path(program)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    _ = resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )

    initialize_manager_file_logging(program.home_dir, log_level=program.app_log_level)
    warn_if_log_level_auto_healed(
        program_obj=program,
        program_path=program_path,
        logger=_manager_logger,
    )

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
