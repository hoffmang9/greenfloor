from __future__ import annotations

import collections.abc
import logging
import time
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters import offer_action, rust_signer
from greenfloor.config.models import MarketConfig, ProgramConfig, prepare_signer_runtime
from greenfloor.core.offer_action import (
    OfferCreatePhaseOutcome,
    build_action_request,
    to_create_phase_outcome,
)
from greenfloor.core.offer_bootstrap_bridge import (
    BootstrapPhaseResult,
    plan_bootstrap_mixed_outputs,
)
from greenfloor.core.offer_policy import normalize_offer_side
from greenfloor.core.signer_offer_request import signer_split_asset_id
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.bootstrap_fees import resolve_bootstrap_split_fee
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.offer_bootstrap import (
    BootstrapRuntimeDeps,
    BootstrapSplitExecution,
    ResolveBootstrapSplitFeeFn,
    bootstrap_ladder_entries_for_side,
    execute_bootstrap_mixed_split,
    run_bootstrap_preflight,
)
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import (
    OfferCreateFailure,
    OfferCreateOutcome,
    OfferPostDeps,
    build_and_post_offer,
    default_offer_post_deps,
)

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


def _bootstrap_skip(reason: str) -> BootstrapPhaseResult:
    return BootstrapPhaseResult(status="skipped", reason=reason)


def default_bootstrap_runtime_deps() -> BootstrapRuntimeDeps:
    return BootstrapRuntimeDeps(
        list_bootstrap_coins_fn=_list_coinset_bootstrap_coins,
        wait_for_confirmation_fn=_wait_for_coinset_confirmation,
        is_spendable_coin_fn=is_spendable_coin,
        plan_bootstrap_mixed_outputs_fn=plan_bootstrap_mixed_outputs,
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
    bootstrap_deps: BootstrapRuntimeDeps | None = None,
    resolve_bootstrap_split_fee_fn: ResolveBootstrapSplitFeeFn | None = None,
) -> BootstrapPhaseResult:
    deps = bootstrap_deps or default_bootstrap_runtime_deps()
    resolve_fee_fn = resolve_bootstrap_split_fee_fn or resolve_bootstrap_split_fee

    side = normalize_offer_side(action_side)
    side_ladder = list(market.ladders.get(side, []))
    if not side_ladder:
        return _bootstrap_skip(f"missing_{side}_ladder")

    ladder_entries = bootstrap_ladder_entries_for_side(
        side=side,
        side_ladder=side_ladder,
        pricing=dict(market.pricing or {}),
        quote_price=float(quote_price),
        resolved_quote_asset_id=str(resolved_quote_asset_id),
    )
    if not ladder_entries:
        return _bootstrap_skip(f"empty_{side}_ladder_after_quote_conversion")

    split_asset_id = signer_split_asset_id(
        action_side=action_side,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
    )
    if not split_asset_id:
        return _bootstrap_skip(f"missing_{side}_asset_for_bootstrap")

    receive_address = str(market.receive_address or "").strip()
    if not receive_address:
        return _bootstrap_skip("missing_receive_address_for_bootstrap")

    try:
        asset_scoped_coins = deps.list_bootstrap_coins_fn(
            network=str(program.app_network),
            receive_address=receive_address,
            asset_id=split_asset_id,
        )
    except Exception as exc:
        return _bootstrap_skip(f"bootstrap_coin_list_failed:{exc}")

    spendable_asset_coins = [coin for coin in asset_scoped_coins if deps.is_spendable_coin_fn(coin)]
    preflight_outcome = run_bootstrap_preflight(
        program=program,
        ladder_entries=ladder_entries,
        split_asset_id=split_asset_id,
        receive_address=receive_address,
        spendable_coins=spendable_asset_coins,
        asset_scoped_coins=asset_scoped_coins,
        bootstrap_wait_timeout_seconds=bootstrap_wait_timeout_seconds,
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        deps=deps,
        resolve_bootstrap_split_fee_fn=resolve_fee_fn,
    )
    if preflight_outcome.early is not None:
        return preflight_outcome.early
    if preflight_outcome.preflight is None:
        raise RuntimeError("bootstrap_failed:planner_missing_preflight")

    return execute_bootstrap_mixed_split(
        BootstrapSplitExecution(
            preflight=preflight_outcome.preflight,
            config_path=_signer_config_path(program),
        )
    )


def signer_create_offer_phase(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    action_side: str = "sell",
    split_input_coins: bool = True,
    broadcast_split: bool = True,
) -> OfferCreatePhaseOutcome:
    side = normalize_offer_side(action_side)
    request = build_action_request(
        receive_address=str(market.receive_address or ""),
        base_asset=str(resolved_base_asset_id),
        quote_asset=str(resolved_quote_asset_id),
        pricing=dict(market.pricing or {}),
        size_base_units=int(size_base_units),
        action_side=side,
        quote_price=float(quote_price),
        split_input_coins=split_input_coins,
        broadcast_split=broadcast_split,
    )
    config_path = _signer_config_path(program)
    result = offer_action.build_signer_offer_for_action(config_path, request)
    return to_create_phase_outcome(result, action_side=side)


@dataclass(frozen=True, slots=True)
class SignerOfferDeps:
    post_deps: OfferPostDeps
    resolve_signer_offer_asset_ids_fn: collections.abc.Callable[..., tuple[str, str]]
    signer_bootstrap_phase_fn: collections.abc.Callable[..., BootstrapPhaseResult]
    signer_create_offer_phase_fn: collections.abc.Callable[..., OfferCreatePhaseOutcome]


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

    def bootstrap(**kwargs: Any) -> BootstrapPhaseResult:
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
                action_side=kwargs["action_side"],
            )
        except RuntimeError as exc:
            raise OfferCreateFailure(
                str(exc),
                create_phase_ms=int((time.monotonic() - started) * 1000),
                extra={"signer_path": True},
            ) from exc
        execution_mode = create_phase.execution_mode.strip()
        if not create_phase.offer_text.strip():
            raise OfferCreateFailure(
                "signer_create_offer_failed:missing_offer_text",
                create_phase_ms=int((time.monotonic() - started) * 1000),
                extra={"signer_path": True, "execution_mode": execution_mode},
            )
        return OfferCreateOutcome.from_create_phase(
            create_phase,
            create_phase_ms=int((time.monotonic() - started) * 1000),
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
        path_label="signer",
        path_extra_fields={"signer_path": True},
        post_deps=resolved_deps.post_deps,
        emit_output=emit_output,
        persist_results=persist_results,
    )
