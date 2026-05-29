"""Local BLS offer-build helpers for CLI orchestration."""

from __future__ import annotations

import collections.abc
from pathlib import Path
from typing import Any

from greenfloor.adapters.offer_action import build_bls_offer_from_build_context
from greenfloor.core.offer_action import (
    expires_at_iso_from_build_context,
    to_create_phase_outcome,
)
from greenfloor.core.planned_action import PlannedAction, planned_action_side
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import OfferCreateFailure, OfferCreateOutcome


def build_daemon_action_offer_payload(
    build_ctx: OfferBuildContext,
    *,
    action: Any,
    xch_price_usd: float | None,
) -> dict[str, Any]:
    side = (
        planned_action_side(action)
        if isinstance(action, PlannedAction)
        else str(getattr(action, "side", "sell"))
    )
    payload = build_local_offer_payload(
        build_ctx,
        size_base_units=int(action.size),
        quote_price=float(build_ctx.quote_price),
    )
    payload.update(
        {
            "pair": action.pair,
            "reason": action.reason,
            "side": side,
            "xch_price_usd": xch_price_usd,
            "target_spread_bps": action.target_spread_bps,
            "expiry_unit": action.expiry_unit,
            "expiry_value": int(action.expiry_value),
        }
    )
    return payload


def build_local_offer_payload(
    build_ctx: OfferBuildContext,
    *,
    size_base_units: int,
    quote_price: float,
    dry_run: bool = False,
) -> dict[str, Any]:
    program = build_ctx.program
    market = build_ctx.market
    return {
        "market_id": market.market_id,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": build_ctx.resolved_quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "size_base_units": int(size_base_units),
        "pair": str(build_ctx.resolved_quote_asset).strip().lower(),
        "reason": "manual_build_and_post",
        "xch_price_usd": None,
        "expiry_unit": build_ctx.expiry_unit,
        "expiry_value": int(build_ctx.expiry_value),
        "quote_price_quote_per_base": float(quote_price),
        "base_unit_mojo_multiplier": int(build_ctx.base_unit_mojo_multiplier),
        "quote_unit_mojo_multiplier": int(build_ctx.quote_unit_mojo_multiplier),
        "fee_mojos": 0,
        "dry_run": bool(dry_run),
        "key_id": market.signer_key_id,
        "keyring_yaml_path": build_ctx.keyring_yaml_path,
        "network": build_ctx.network,
        "asset_id": market.base_asset,
        "program_config_path": str(build_ctx.program_path),
        "program_home_dir": str(program.home_dir),
    }


def make_local_offer_create_fn(
    build_ctx: OfferBuildContext,
    *,
    dry_run: bool,
    capture_dir_path: Path | None = None,
    build_offer_fn: collections.abc.Callable[[dict[str, Any]], str] | None = None,
) -> collections.abc.Callable[..., OfferCreateOutcome]:
    offer_iteration = [0]

    def create(**kwargs: Any) -> OfferCreateOutcome:
        index = offer_iteration[0]
        offer_iteration[0] += 1
        try:
            expires_at = expires_at_iso_from_build_context(
                expiry_unit=build_ctx.expiry_unit,
                expiry_value=int(build_ctx.expiry_value),
            )
            if build_offer_fn is not None:
                payload = build_local_offer_payload(
                    build_ctx,
                    size_base_units=int(kwargs["size_base_units"]),
                    quote_price=float(kwargs["quote_price"]),
                    dry_run=dry_run,
                )
                offer_text = build_offer_fn(payload)
            else:
                action_result = build_bls_offer_from_build_context(
                    build_ctx,
                    size_base_units=int(kwargs["size_base_units"]),
                    quote_price=float(kwargs["quote_price"]),
                    action_side=str(kwargs.get("action_side", build_ctx.action_side)),
                )
                outcome = to_create_phase_outcome(
                    action_result,
                    action_side=str(kwargs.get("action_side", build_ctx.action_side)),
                )
                offer_text = outcome.offer_text
                expires_at = outcome.expires_at
        except Exception as exc:
            raise OfferCreateFailure(f"offer_builder_failed:{exc}") from exc

        extra: dict[str, Any] = {}
        if dry_run and capture_dir_path is not None:
            capture_file = (
                capture_dir_path / f"{build_ctx.market.market_id}-dry-run-{index + 1}.offer"
            )
            capture_file.write_text(offer_text, encoding="utf-8")
            extra["dry_run_preview"] = {"offer_capture_path": str(capture_file)}

        return OfferCreateOutcome(
            offer_text=offer_text,
            expires_at=expires_at,
            side=str(kwargs.get("action_side", build_ctx.action_side)),
            extra=extra,
        )

    return create
