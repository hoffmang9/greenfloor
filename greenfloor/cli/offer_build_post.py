"""CLI build-and-post offer commands."""

from __future__ import annotations

import logging
import os
from pathlib import Path

from greenfloor.config.io import (
    is_testnet,
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
)
from greenfloor.config.models import offer_execution_backend
from greenfloor.logging_setup import warn_if_log_level_auto_healed
from greenfloor.offer_builder import build_offer
from greenfloor.runtime.json_output import format_json_output
from greenfloor.runtime.offer_build_context import prepare_offer_build_context
from greenfloor.runtime.offer_execution import default_offer_post_deps
from greenfloor.runtime.offer_post_request import OfferPostRequest
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
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    market = resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    build_ctx = prepare_offer_build_context(
        program=program,
        market=market,
        program_path=program_path,
        network=network,
    )

    initialize_manager_file_logging(program.home_dir, log_level=program.app_log_level)
    warn_if_log_level_auto_healed(
        program_obj=program,
        program_path=program_path,
        logger=_manager_logger,
    )

    request = OfferPostRequest(
        build_ctx=build_ctx,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        dry_run=bool(dry_run),
    )

    debug_dry_run_offer_capture_dir = os.getenv(
        "GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", ""
    ).strip()
    capture_dir_path = (
        Path(debug_dry_run_offer_capture_dir).expanduser()
        if debug_dry_run_offer_capture_dir
        else None
    )
    if dry_run and capture_dir_path is not None:
        capture_dir_path.mkdir(parents=True, exist_ok=True)

    backend = offer_execution_backend(program, size_base_units=size_base_units)
    return request.run_cli(
        backend,
        capture_dir_path=capture_dir_path,
        build_offer_fn=build_offer,
        post_deps=default_offer_post_deps(format_output_fn=format_json_output),
        path_extra_fields={"local_cli_path": True},
    )
