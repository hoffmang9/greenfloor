"""CLI build-and-post offer commands."""

from __future__ import annotations

import logging
import os
from pathlib import Path

from greenfloor.config.io import (
    is_testnet,
    load_markets_config_with_optional_overlay,
    load_program_config,
)
from greenfloor.config.models import MarketsConfig, offer_execution_backend, prepare_signer_runtime
from greenfloor.logging_setup import warn_if_log_level_auto_healed
from greenfloor.offer_builder import build_offer_text
from greenfloor.runtime.cloud_wallet.adapter import _format_json_output
from greenfloor.runtime.offer_execution import (
    BootstrapPolicy,
    build_and_post_offer,
    build_and_post_offer_cloud_wallet,
    build_and_post_offer_signer,
    default_offer_post_deps,
    local_offer_params_from_context,
    make_local_offer_create_fn,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_publish import initialize_manager_file_logging

_manager_logger = logging.getLogger("greenfloor.manager")


def resolve_market_for_build(
    markets: MarketsConfig,
    *,
    market_id: str | None,
    pair: str | None,
    network: str,
):
    if bool(market_id) == bool(pair):
        raise ValueError("provide exactly one of --market-id or --pair")
    if market_id:
        selected = next((m for m in markets.markets if m.market_id == market_id), None)
        if selected is None:
            raise ValueError(f"market_id not found: {market_id}")
        return selected

    assert pair is not None
    raw = pair.strip()
    sep = ":" if ":" in raw else "/" if "/" in raw else ""
    if not sep:
        raise ValueError("pair must be in base:quote or base/quote format")
    base_raw, quote_raw = [p.strip().lower() for p in raw.split(sep, 1)]
    if not base_raw or not quote_raw:
        raise ValueError("pair base and quote must be non-empty")
    network_l = network.strip().lower()
    candidates = []
    for market in markets.markets:
        if not market.enabled:
            continue
        base_matches = {
            str(market.base_asset).strip().lower(),
            str(market.base_symbol).strip().lower(),
        }
        quote_match = str(market.quote_asset).strip().lower()
        quote_matches = {quote_match}
        if is_testnet(network_l):
            if quote_match == "xch":
                quote_matches.add("txch")
            elif quote_match == "txch":
                quote_matches.add("xch")
        if base_raw in base_matches and quote_raw in quote_matches:
            candidates.append(market)
    if not candidates:
        raise ValueError(f"no enabled market found for pair: {pair}")
    if len(candidates) > 1:
        ids = ", ".join(sorted(m.market_id for m in candidates))
        raise ValueError(f"pair is ambiguous; use --market-id (candidates: {ids})")
    return candidates[0]


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
    signer_key = program.signer_key_registry.get(market.signer_key_id)
    keyring_yaml_path = str(signer_key.keyring_yaml_path or "") if signer_key is not None else ""
    build_ctx = prepare_offer_build_context(
        program=program,
        market=market,
        program_path=program_path,
        network=network,
        keyring_yaml_path=keyring_yaml_path,
    )

    initialize_manager_file_logging(program.home_dir, log_level=program.app_log_level)
    warn_if_log_level_auto_healed(
        program_obj=program,
        program_path=program_path,
        logger=_manager_logger,
    )

    backend = offer_execution_backend(program, size_base_units=size_base_units)
    if backend == "signer":
        prepare_signer_runtime(program)
        exit_code, _ = build_and_post_offer_signer(
            program=program,
            market=market,
            size_base_units=size_base_units,
            repeat=repeat,
            publish_venue=publish_venue,
            dexie_base_url=dexie_base_url,
            splash_base_url=splash_base_url,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
            quote_price=build_ctx.quote_price,
            dry_run=bool(dry_run),
        )
        return exit_code
    if backend == "cloud_wallet":
        exit_code, _ = build_and_post_offer_cloud_wallet(
            program=program,
            market=market,
            size_base_units=size_base_units,
            repeat=repeat,
            publish_venue=publish_venue,
            dexie_base_url=dexie_base_url,
            splash_base_url=splash_base_url,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
            quote_price=build_ctx.quote_price,
            dry_run=bool(dry_run),
        )
        return exit_code

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

    local_params = local_offer_params_from_context(
        build_ctx,
        dry_run=bool(dry_run),
        capture_dir_path=capture_dir_path,
    )

    exit_code, _ = build_and_post_offer(
        program=program,
        market=market,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        quote_price=build_ctx.quote_price,
        dry_run=bool(dry_run),
        action_side=build_ctx.action_side,
        resolved_base_asset_id=str(market.base_asset),
        resolved_quote_asset_id=build_ctx.resolved_quote_asset,
        bootstrap_phase_fn=None,
        create_offer_fn=make_local_offer_create_fn(
            local_params,
            build_offer_text_fn=build_offer_text,
        ),
        bootstrap_policy=BootstrapPolicy(allow_split_fallback=False),
        path_label="local",
        path_extra_fields={"local_cli_path": True},
        post_deps=default_offer_post_deps(format_output_fn=_format_json_output),
    )
    return exit_code
