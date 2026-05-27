"""Local BLS offer-build helpers for CLI orchestration."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import OfferCreateFailure, OfferCreateOutcome
from greenfloor.runtime.offer_publish import normalize_offer_side


@dataclass(frozen=True, slots=True)
class LocalOfferBuildParams:
    program: ProgramConfig
    market: MarketConfig
    program_path: Path
    network: str
    resolved_quote_asset: str
    expiry_unit: str
    expiry_value: int
    base_unit_mojo_multiplier: int
    quote_unit_mojo_multiplier: int
    keyring_yaml_path: str
    dry_run: bool
    action_side: str = "sell"
    capture_dir_path: Path | None = None


def local_offer_params_from_context(
    build_ctx: OfferBuildContext,
    *,
    dry_run: bool,
    capture_dir_path: Path | None = None,
) -> LocalOfferBuildParams:
    return LocalOfferBuildParams(
        program=build_ctx.program,
        market=build_ctx.market,
        program_path=build_ctx.program_path,
        network=build_ctx.network,
        resolved_quote_asset=build_ctx.resolved_quote_asset,
        expiry_unit=build_ctx.expiry_unit,
        expiry_value=build_ctx.expiry_value,
        base_unit_mojo_multiplier=build_ctx.base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier=build_ctx.quote_unit_mojo_multiplier,
        keyring_yaml_path=build_ctx.keyring_yaml_path,
        dry_run=bool(dry_run),
        action_side=build_ctx.action_side,
        capture_dir_path=capture_dir_path,
    )


def build_local_offer_payload(
    params: LocalOfferBuildParams,
    *,
    size_base_units: int,
    quote_price: float,
) -> dict[str, Any]:
    program = params.program
    market = params.market
    return {
        "market_id": market.market_id,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": params.resolved_quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "size_base_units": int(size_base_units),
        "pair": str(params.resolved_quote_asset).strip().lower(),
        "reason": "manual_build_and_post",
        "xch_price_usd": None,
        "expiry_unit": params.expiry_unit,
        "expiry_value": int(params.expiry_value),
        "quote_price_quote_per_base": float(quote_price),
        "base_unit_mojo_multiplier": int(params.base_unit_mojo_multiplier),
        "quote_unit_mojo_multiplier": int(params.quote_unit_mojo_multiplier),
        "fee_mojos": 0,
        "dry_run": bool(params.dry_run),
        "key_id": market.signer_key_id,
        "keyring_yaml_path": params.keyring_yaml_path,
        "network": params.network,
        "asset_id": market.base_asset,
        "offer_coin_ids": [],
        "cloud_wallet_base_url": str(program.cloud_wallet_base_url or "").strip(),
        "cloud_wallet_user_key_id": str(program.cloud_wallet_user_key_id or "").strip(),
        "cloud_wallet_private_key_pem_path": str(
            program.cloud_wallet_private_key_pem_path or ""
        ).strip(),
        "cloud_wallet_vault_id": str(program.cloud_wallet_vault_id or "").strip(),
        "cloud_wallet_kms_key_id": str(program.cloud_wallet_kms_key_id or "").strip(),
        "cloud_wallet_kms_region": str(program.cloud_wallet_kms_region or "").strip(),
        "cloud_wallet_kms_public_key_hex": str(
            program.cloud_wallet_kms_public_key_hex or ""
        ).strip(),
        "program_config_path": str(params.program_path),
        "program_home_dir": str(program.home_dir),
    }


def make_local_offer_create_fn(
    params: LocalOfferBuildParams,
    *,
    build_offer_text_fn: collections.abc.Callable[[dict[str, Any]], str],
) -> collections.abc.Callable[..., OfferCreateOutcome]:
    offer_iteration = [0]

    def create(**kwargs: Any) -> OfferCreateOutcome:
        index = offer_iteration[0]
        offer_iteration[0] += 1
        payload = build_local_offer_payload(
            params,
            size_base_units=int(kwargs["size_base_units"]),
            quote_price=float(kwargs["quote_price"]),
        )
        try:
            offer_text = build_offer_text_fn(payload)
        except Exception as exc:
            raise OfferCreateFailure(f"offer_builder_failed:{exc}") from exc

        extra: dict[str, Any] = {}
        if params.dry_run and params.capture_dir_path is not None:
            capture_file = (
                params.capture_dir_path / f"{params.market.market_id}-dry-run-{index + 1}.offer"
            )
            capture_file.write_text(offer_text, encoding="utf-8")
            extra["dry_run_preview"] = {"offer_capture_path": str(capture_file)}

        return OfferCreateOutcome(
            offer_text=offer_text,
            expires_at=f"{int(params.expiry_value)} {params.expiry_unit}",
            side=normalize_offer_side(kwargs.get("action_side", params.action_side)),
            extra=extra,
        )

    return create
