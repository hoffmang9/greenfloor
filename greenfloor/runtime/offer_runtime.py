from __future__ import annotations

import collections.abc
import datetime as dt
import logging
import time
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters import rust_signer
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig, prepare_signer_runtime
from greenfloor.hex_utils import canonical_is_xch, default_mojo_multiplier_for_asset
from greenfloor.offer_bootstrap import BootstrapLadderEntry, plan_bootstrap_mixed_outputs
from greenfloor.runtime.cloud_wallet.bootstrap import resolve_bootstrap_split_fee
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import (
    BootstrapPolicy,
    OfferCreateFailure,
    OfferCreateOutcome,
    OfferPostDeps,
    build_and_post_offer,
    default_offer_post_deps,
)
from greenfloor.runtime.offer_publish import normalize_offer_side

_runtime_logger = logging.getLogger("greenfloor.manager")


def _signer_config_path(program: ProgramConfig) -> str:
    return prepare_signer_runtime(program)


def signer_resolve_offer_asset_ids(
    *,
    program: ProgramConfig,
    base_asset_id: str,
    quote_asset_id: str,
) -> tuple[str, str]:
    config_path = _signer_config_path(program)
    payload = rust_signer.resolve_offer_asset_ids(
        config_path,
        str(base_asset_id).strip(),
        str(quote_asset_id).strip(),
    )
    resolved_base = str(payload.get("base_asset_id", "")).strip()
    resolved_quote = str(payload.get("quote_asset_id", "")).strip()
    if not resolved_base or not resolved_quote:
        raise RuntimeError("signer_asset_resolution_failed:empty_resolved_asset_id")
    if (
        resolved_base == resolved_quote
        and not canonical_is_xch(base_asset_id)
        and not canonical_is_xch(quote_asset_id)
    ):
        raise RuntimeError(
            "signer_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair"
        )
    return resolved_base, resolved_quote


def _list_coinset_bootstrap_coins(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
) -> list[dict[str, Any]]:
    from greenfloor.runtime.coinset_coins import list_unspent_coins_by_receive_address

    return list_unspent_coins_by_receive_address(
        network=network,
        receive_address=receive_address,
        asset_id=asset_id,
    )


def _wait_for_coinset_confirmation(
    *,
    network: str,
    receive_address: str,
    asset_id: str,
    initial_coin_ids: set[str],
    timeout_seconds: int,
) -> list[dict[str, str]]:
    from greenfloor.runtime.coinset_coins import wait_for_coinset_confirmation

    return wait_for_coinset_confirmation(
        network=network,
        receive_address=receive_address,
        asset_id=asset_id,
        initial_coin_ids=initial_coin_ids,
        timeout_seconds=timeout_seconds,
    )


def signer_bootstrap_phase(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    quote_price: float,
    action_side: str = "sell",
    bootstrap_wait_timeout_seconds: int = 120,
    plan_bootstrap_mixed_outputs_fn: collections.abc.Callable[..., Any] | None = None,
    resolve_bootstrap_split_fee_fn: collections.abc.Callable[..., tuple[int, str, str | None]]
    | None = None,
    list_bootstrap_coins_fn: collections.abc.Callable[..., list[dict[str, Any]]] | None = None,
    wait_for_confirmation_fn: collections.abc.Callable[..., list[dict[str, str]]] | None = None,
    is_spendable_coin_fn: collections.abc.Callable[[dict], bool] | None = None,
) -> dict[str, Any]:
    if plan_bootstrap_mixed_outputs_fn is None:
        plan_bootstrap_mixed_outputs_fn = plan_bootstrap_mixed_outputs
    if resolve_bootstrap_split_fee_fn is None:
        resolve_bootstrap_split_fee_fn = resolve_bootstrap_split_fee
    if list_bootstrap_coins_fn is None:
        list_bootstrap_coins_fn = _list_coinset_bootstrap_coins
    if wait_for_confirmation_fn is None:
        wait_for_confirmation_fn = _wait_for_coinset_confirmation
    if is_spendable_coin_fn is None:
        is_spendable_coin_fn = is_spendable_coin

    side = normalize_offer_side(action_side)
    ladders = market.ladders or {}
    side_ladder = list(ladders.get(side, []) or []) if isinstance(ladders, dict) else []
    if not side_ladder:
        return {"status": "skipped", "reason": f"missing_{side}_ladder"}

    pricing = dict(market.pricing or {})
    quote_unit_multiplier = int(
        pricing.get(
            "quote_unit_mojo_multiplier",
            default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
        )
    )
    if side == "buy":
        ladder_for_split = []
        for entry in side_ladder:
            quote_amount = int(
                round(float(entry.size_base_units) * float(quote_price) * quote_unit_multiplier)
            )
            if quote_amount <= 0:
                continue
            ladder_for_split.append(
                BootstrapLadderEntry(
                    size_base_units=quote_amount,
                    target_count=int(entry.target_count),
                    split_buffer_count=int(entry.split_buffer_count),
                )
            )
        split_asset_id = str(resolved_quote_asset_id).strip()
    else:
        ladder_for_split = side_ladder
        split_asset_id = str(resolved_base_asset_id).strip()

    if not split_asset_id:
        return {"status": "skipped", "reason": f"missing_{side}_asset_for_bootstrap"}

    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        return {"status": "skipped", "reason": "missing_receive_address_for_bootstrap"}

    try:
        asset_scoped_coins = list_bootstrap_coins_fn(
            network=str(program.app_network),
            receive_address=receive_address,
            asset_id=split_asset_id,
        )
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"bootstrap_coin_list_failed:{exc}",
        }

    spendable_asset_coins = [coin for coin in asset_scoped_coins if is_spendable_coin_fn(coin)]
    bootstrap_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=spendable_asset_coins,
    )
    if bootstrap_plan is None:
        return {"status": "skipped", "reason": "already_ready"}

    fee_mojos, fee_source, fee_lookup_error = resolve_bootstrap_split_fee_fn(
        network=str(program.app_network),
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        output_count=len(bootstrap_plan.output_amounts_base_units),
    )
    if int(fee_mojos) > 0:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:signer_mixed_split_fee_not_supported",
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }

    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in asset_scoped_coins if str(c.get("id", "")).strip()
    }
    selected_deficit = max(
        bootstrap_plan.deficits,
        key=lambda row: (int(row.size_base_units), int(row.deficit_count)),
    )
    amount_per_coin = int(selected_deficit.size_base_units)
    desired_coin_count = max(2, int(selected_deficit.deficit_count))
    max_coin_count = int(bootstrap_plan.source_amount) // max(1, amount_per_coin)
    number_of_coins = min(desired_coin_count, max_coin_count)
    if number_of_coins < 2:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:insufficient_source_coin_for_signer_split",
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }

    output_amounts = [amount_per_coin] * number_of_coins
    config_path = _signer_config_path(program)
    split_request = {
        "receive_address": receive_address,
        "asset_id": split_asset_id.removeprefix("0x"),
        "output_amounts": output_amounts,
        "coin_ids": [bootstrap_plan.source_coin_id.removeprefix("0x")],
        "allow_sub_cat_output": False,
        "fee_mojos": 0,
        "broadcast": True,
    }
    try:
        split_result = rust_signer.build_mixed_split(config_path, split_request)
    except Exception as exc:
        return {
            "status": "failed",
            "reason": f"bootstrap_failed:signer_mixed_split_error:{exc}",
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }

    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = wait_for_confirmation_fn(
            network=str(program.app_network),
            receive_address=receive_address,
            asset_id=split_asset_id,
            initial_coin_ids=existing_coin_ids,
            timeout_seconds=max(10, int(bootstrap_wait_timeout_seconds)),
        )
    except Exception as exc:
        wait_error = str(exc)
        return {
            "status": "failed",
            "reason": "bootstrap_wait_failed",
            "wait_error": wait_error,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "split_result": dict(split_result) if isinstance(split_result, dict) else {},
            "wait_events": wait_events,
        }

    refreshed_asset_coins = list_bootstrap_coins_fn(
        network=str(program.app_network),
        receive_address=receive_address,
        asset_id=split_asset_id,
    )
    refreshed_spendable = [coin for coin in refreshed_asset_coins if is_spendable_coin_fn(coin)]
    remaining_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=refreshed_spendable,
    )
    return {
        "status": "executed",
        "reason": "bootstrap_submitted",
        "ready": remaining_plan is None,
        "fee_mojos": int(fee_mojos),
        "fee_source": fee_source,
        "fee_lookup_error": fee_lookup_error,
        "wait_error": wait_error,
        "split_result": dict(split_result) if isinstance(split_result, dict) else {},
        "wait_events": wait_events,
        "plan": {
            "source_coin_id": bootstrap_plan.source_coin_id,
            "source_amount": bootstrap_plan.source_amount,
            "output_count": len(output_amounts),
            "total_output_amount": bootstrap_plan.total_output_amount,
            "change_amount": bootstrap_plan.change_amount,
        },
    }


def signer_create_offer_phase(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    expiry_unit: str,
    expiry_value: int,
    action_side: str = "sell",
    split_input_coins: bool = True,
    broadcast_split: bool = True,
) -> dict[str, Any]:
    side = normalize_offer_side(action_side)
    offer_amount = int(
        size_base_units
        * int(
            (market.pricing or {}).get(
                "base_unit_mojo_multiplier",
                default_mojo_multiplier_for_asset(str(resolved_base_asset_id)),
            )
        )
    )
    request_amount = int(
        round(
            float(size_base_units)
            * float(quote_price)
            * int(
                (market.pricing or {}).get(
                    "quote_unit_mojo_multiplier",
                    default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
                )
            )
        )
    )
    if request_amount <= 0:
        raise ValueError("request_amount must be positive")

    if side == "buy":
        offer_asset_id = str(resolved_quote_asset_id).strip()
        request_asset_id = str(resolved_base_asset_id).strip()
        offer_amount_mojos = request_amount
        request_amount_mojos = offer_amount
    else:
        offer_asset_id = str(resolved_base_asset_id).strip()
        request_asset_id = str(resolved_quote_asset_id).strip()
        offer_amount_mojos = offer_amount
        request_amount_mojos = request_amount

    expires_at_dt = dt.datetime.now(dt.UTC) + dt.timedelta(**{expiry_unit: int(expiry_value)})
    expires_at_unix = int(expires_at_dt.timestamp())
    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        raise ValueError("market.receive_address is required for signer offer build")

    config_path = _signer_config_path(program)
    request = {
        "receive_address": receive_address,
        "offer_asset_id": offer_asset_id.removeprefix("0x"),
        "offer_amount": int(offer_amount_mojos),
        "request_asset_id": request_asset_id.removeprefix("0x"),
        "request_amount": int(request_amount_mojos),
        "offer_coin_ids": [],
        "presplit_coin_ids": [],
        "split_input_coins": bool(split_input_coins),
        "broadcast_split": bool(broadcast_split),
        "expires_at": expires_at_unix,
    }
    result = rust_signer.build_vault_cat_offer(config_path, request)
    offer_text = str(result.get("offer", "")).strip()
    if not offer_text.startswith("offer1"):
        raise RuntimeError("signer_create_offer_failed:missing_offer_text")
    return {
        "offer_text": offer_text,
        "expires_at": expires_at_dt.isoformat(),
        "offer_amount": offer_amount,
        "request_amount": request_amount,
        "side": side,
        "execution_mode": str(result.get("execution_mode", "")).strip(),
        "create_result": dict(result) if isinstance(result, dict) else {},
    }


@dataclass(frozen=True, slots=True)
class SignerOfferDeps:
    post_deps: OfferPostDeps
    resolve_signer_offer_asset_ids_fn: collections.abc.Callable[..., tuple[str, str]]
    signer_bootstrap_phase_fn: collections.abc.Callable[..., dict[str, Any]]
    signer_create_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]]


def default_signer_offer_deps(*, post_deps: OfferPostDeps | None = None) -> SignerOfferDeps:
    return SignerOfferDeps(
        post_deps=post_deps or default_offer_post_deps(),
        resolve_signer_offer_asset_ids_fn=signer_resolve_offer_asset_ids,
        signer_bootstrap_phase_fn=signer_bootstrap_phase,
        signer_create_offer_phase_fn=signer_create_offer_phase,
    )


def build_and_post_offer_signer(
    *,
    build_ctx: OfferBuildContext,
    size_base_units: int,
    repeat: int,
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
    deps: SignerOfferDeps | None = None,
    emit_output: bool = True,
    persist_results: bool = True,
) -> tuple[int, dict[str, Any]]:
    program = build_ctx.program
    market = build_ctx.market
    resolved_deps = deps or default_signer_offer_deps()

    prepare_signer_runtime(program)
    resolved_base_asset_id, resolved_quote_asset_id = (
        resolved_deps.resolve_signer_offer_asset_ids_fn(
            program=program,
            base_asset_id=str(market.base_asset),
            quote_asset_id=str(market.quote_asset),
        )
    )
    expiry_unit = build_ctx.expiry_unit
    expiry_value = int(build_ctx.expiry_value)

    def bootstrap(**kwargs: Any) -> dict[str, Any]:
        return resolved_deps.signer_bootstrap_phase_fn(
            bootstrap_wait_timeout_seconds=int(
                program.runtime_offer_bootstrap_wait_timeout_seconds
            ),
            **kwargs,
        )

    def create(**kwargs: Any) -> OfferCreateOutcome:
        started = time.monotonic()
        try:
            create_phase = resolved_deps.signer_create_offer_phase_fn(
                program=program,
                market=kwargs["market"],
                size_base_units=kwargs["size_base_units"],
                quote_price=kwargs["quote_price"],
                resolved_base_asset_id=kwargs["resolved_base_asset_id"],
                resolved_quote_asset_id=kwargs["resolved_quote_asset_id"],
                expiry_unit=expiry_unit,
                expiry_value=expiry_value,
                action_side=kwargs["action_side"],
            )
        except RuntimeError as exc:
            raise OfferCreateFailure(
                str(exc),
                create_phase_ms=int((time.monotonic() - started) * 1000),
                extra={"signer_path": True},
            ) from exc
        execution_mode = str(create_phase.get("execution_mode", "")).strip()
        offer_text = str(create_phase.get("offer_text", "")).strip()
        if not offer_text:
            raise OfferCreateFailure(
                "signer_create_offer_failed:missing_offer_text",
                create_phase_ms=int((time.monotonic() - started) * 1000),
                extra={"signer_path": True, "execution_mode": execution_mode},
            )
        return OfferCreateOutcome(
            offer_text=str(create_phase.get("offer_text", "")).strip(),
            expires_at=str(create_phase.get("expires_at", "")),
            side=str(create_phase.get("side", kwargs["action_side"])),
            create_phase_ms=int((time.monotonic() - started) * 1000),
            extra={"execution_mode": execution_mode} if execution_mode else {},
        )

    return build_and_post_offer(
        build_ctx=build_ctx,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        dry_run=dry_run,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        bootstrap_phase_fn=bootstrap,
        create_offer_fn=create,
        bootstrap_policy=BootstrapPolicy(allow_split_fallback=False),
        path_label="signer",
        path_extra_fields={"signer_path": True},
        post_deps=resolved_deps.post_deps,
        emit_output=emit_output,
        persist_results=persist_results,
    )
