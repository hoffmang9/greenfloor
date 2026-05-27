"""Shared Cloud Wallet coin-operation runtime (CLI, daemon refresh, and tests)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_market_for_build,
)
from greenfloor.config.models import MarketConfig, MarketLadderEntry, ProgramConfig
from greenfloor.runtime import coinset_runtime
from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet import assets as cloud_wallet_assets
from greenfloor.runtime.cloud_wallet import polling as cloud_wallet_polling
from greenfloor.runtime.cloud_wallet.adapter import (
    _require_cloud_wallet_config as require_cloud_wallet_config,
)
from greenfloor.runtime.cloud_wallet.coin_op_errors import coin_op_error_payload
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    DenominationTarget,
    denomination_target_payload,
)
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.coin_ops_backend import (
    CloudWalletCoinOpBackend,
    CoinOpBackend,
    CoinOpScope,
    SignerCoinOpBackend,
    build_coin_op_backend,
    resolve_coin_op_base_asset_id,
    resolve_signer_asset_id,
    scope_payload,
)
from greenfloor.runtime.coinset_runtime import CoinsetFeeLookupPreflightError


@dataclass
class CoinOpDeps:
    """Test/DI seam: methods delegate at call time so tests can monkeypatch modules."""

    def new_cloud_wallet_adapter(self, program: ProgramConfig) -> CloudWalletAdapter:
        return cloud_wallet_adapter.new_cloud_wallet_adapter(program)

    def resolve_cloud_wallet_asset_id(self, **kwargs: Any) -> str:
        return cloud_wallet_assets.resolve_cloud_wallet_asset_id(**kwargs)

    def resolve_taker_or_coin_operation_fee(self, **kwargs: Any) -> tuple[int, str]:
        return coinset_runtime._resolve_taker_or_coin_operation_fee(**kwargs)

    def poll_signature_request_until_not_unsigned(self, **kwargs: Any) -> tuple[str, list]:
        return cloud_wallet_polling.poll_signature_request_until_not_unsigned(**kwargs)

    def wait_for_mempool_then_confirmation(self, **kwargs: Any) -> list:
        return cloud_wallet_polling.wait_for_mempool_then_confirmation(**kwargs)


DEFAULT_COIN_OP_DEPS = CoinOpDeps()


def wallet_with_optional_vault_override(
    program: ProgramConfig,
    *,
    vault_id: str | None,
    deps: CoinOpDeps = DEFAULT_COIN_OP_DEPS,
) -> CloudWalletAdapter:
    wallet = deps.new_cloud_wallet_adapter(program)
    if vault_id and vault_id.strip() and vault_id.strip() != wallet.vault_id:
        override_config = require_cloud_wallet_config(program)
        wallet = cloud_wallet_adapter.CloudWalletAdapter(
            CloudWalletConfig(
                base_url=override_config.base_url,
                user_key_id=override_config.user_key_id,
                private_key_pem_path=override_config.private_key_pem_path,
                vault_id=vault_id.strip(),
                network=override_config.network,
            )
        )
    return wallet


def evaluate_denomination_readiness(
    *,
    wallet: CloudWalletAdapter,
    asset_id: str,
    size_base_units: int,
    required_min_count: int | None = None,
    max_allowed_count: int | None = None,
) -> dict[str, int | bool | str]:
    from greenfloor.runtime.cloud_wallet.coins import coin_asset_id

    coins = wallet.list_coins(include_pending=True)
    spendable = [
        c
        for c in coins
        if is_spendable_coin(c)
        and coin_asset_id(c).lower() == asset_id.strip().lower()
        and int(c.get("amount", 0)) == int(size_base_units)
    ]
    current_count = len(spendable)
    ready = True
    if required_min_count is not None:
        ready = current_count >= int(required_min_count)
    if max_allowed_count is not None:
        ready = ready and current_count <= int(max_allowed_count)
    return {
        "asset_id": asset_id,
        "size_base_units": int(size_base_units),
        "current_count": current_count,
        "required_min_count": int(required_min_count) if required_min_count is not None else -1,
        "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
        "ready": ready,
    }


def as_wait_events(value: object) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    items: list[dict[str, str]] = []
    for row in value:
        if isinstance(row, dict):
            event = {str(k): str(v) for k, v in row.items()}
            items.append(event)
    return items


@dataclass(slots=True)
class CoinOpFeeResult:
    fee_mojos: int = 0
    fee_source: str = ""
    error_payload: dict[str, object] | None = None

    @property
    def ok(self) -> bool:
        return self.error_payload is None


def resolve_coin_op_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    market: MarketConfig,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    deps: CoinOpDeps = DEFAULT_COIN_OP_DEPS,
) -> CoinOpFeeResult:
    """Resolve fee for a coin operation without printing."""
    try:
        fee_mojos, fee_source = deps.resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
        )
        return CoinOpFeeResult(fee_mojos=int(fee_mojos), fee_source=str(fee_source))
    except CoinsetFeeLookupPreflightError as exc:
        operator_guidance = (
            "verify Coinset endpoint routing: unset GREENFLOOR_COINSET_BASE_URL to use "
            "network defaults, or set it to a valid endpoint for the active network"
            if exc.failure_kind == "endpoint_validation_failed"
            else "coinset fee advice is temporarily unavailable; retry shortly and verify Coinset fee endpoint health before resubmitting"
        )
        return CoinOpFeeResult(
            error_payload=coin_op_error_payload(
                scope=CoinOpScope(
                    market=market,
                    selected_venue=selected_venue,
                    execution_backend="cloud_wallet",
                    vault_id=str(wallet.vault_id),
                ),
                error=f"coinset_fee_preflight_failed:{exc.failure_kind}",
                operator_guidance=operator_guidance,
                coinset_fee_lookup={
                    "status": "failed",
                    "failure_kind": exc.failure_kind,
                    "detail": exc.detail,
                    **exc.diagnostics,
                },
            )
        )
    except Exception as exc:
        return CoinOpFeeResult(
            error_payload=coin_op_error_payload(
                scope=CoinOpScope(
                    market=market,
                    selected_venue=selected_venue,
                    execution_backend="cloud_wallet",
                    vault_id=str(wallet.vault_id),
                ),
                error=f"fee_resolution_failed:{exc}",
                operator_guidance=(
                    "set coin_ops.minimum_fee_mojos in program config (can be 0) "
                    "or fix GREENFLOOR_COINSET_BASE_URL to a valid Coinset API endpoint"
                ),
            )
        )


def coin_op_build_iteration_payload(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    initial_signature_state: str,
    no_wait: bool,
    network: str,
    existing_coin_ids: set[str],
    iteration: int,
    denomination_target: DenominationTarget,
    readiness_asset_id: str,
    readiness_kwargs: dict[str, int],
    deps: CoinOpDeps = DEFAULT_COIN_OP_DEPS,
) -> tuple[dict[str, object], dict[str, int | bool | str] | None]:
    wait_events: list[dict[str, str]] = []
    final_signature_state = initial_signature_state
    if not no_wait:
        final_signature_state, signature_events = deps.poll_signature_request_until_not_unsigned(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=15 * 60,
            warning_interval_seconds=10 * 60,
        )
        wait_events.extend(signature_events)
        wait_events.extend(
            deps.wait_for_mempool_then_confirmation(
                wallet=wallet,
                network=network,
                initial_coin_ids=existing_coin_ids,
                include_pending=True,
                mempool_warning_seconds=5 * 60,
                confirmation_warning_seconds=15 * 60,
            )
        )
    iteration_payload: dict[str, object] = {
        "iteration": iteration,
        "signature_request_id": signature_request_id,
        "signature_state": final_signature_state,
        "waited": not no_wait,
        "wait_events": wait_events,
    }
    final_readiness = None
    if denomination_target is not None:
        final_readiness = evaluate_denomination_readiness(
            wallet=wallet,
            asset_id=readiness_asset_id,
            size_base_units=denomination_target.size_base_units,
            **readiness_kwargs,
        )
        iteration_payload["denomination_readiness"] = final_readiness
    return iteration_payload, final_readiness


def coin_op_should_stop(
    *,
    until_ready: bool,
    final_readiness: dict[str, int | bool | str] | None,
    coin_ids: list[str],
    iteration: int,
    max_iterations: int,
) -> tuple[bool, str]:
    if not until_ready or final_readiness is None or bool(final_readiness["ready"]):
        stop_reason = "ready" if until_ready and final_readiness is not None else "single_pass"
        return True, stop_reason
    if coin_ids:
        return True, "requires_new_coin_selection"
    if iteration == max_iterations:
        return True, "max_iterations_reached"
    return False, ""


def evaluate_coin_split_gate(
    *,
    asset_scoped_coins: list[dict],
    resolved_asset_id: str,
    size_base_units: int,
    required_count: int,
) -> dict[str, int | bool | str]:
    spendable_asset_coins = [coin for coin in asset_scoped_coins if is_spendable_coin(coin)]
    denom_coins = [
        coin for coin in spendable_asset_coins if int(coin.get("amount", 0)) == int(size_base_units)
    ]
    larger_reserve_coins = [
        coin for coin in spendable_asset_coins if int(coin.get("amount", 0)) > int(size_base_units)
    ]
    current_count = len(denom_coins)
    extra_denom_count = max(0, current_count - int(required_count))
    larger_reserve_count = len(larger_reserve_coins)
    reserve_ready = larger_reserve_count >= 1 or extra_denom_count >= 1
    ready = current_count >= int(required_count) and reserve_ready
    return {
        "asset_id": resolved_asset_id,
        "size_base_units": int(size_base_units),
        "required_min_count": int(required_count),
        "current_count": current_count,
        "larger_reserve_coin_count": larger_reserve_count,
        "extra_denom_coin_count": extra_denom_count,
        "reserve_ready": reserve_ready,
        "ready": ready,
    }


def coin_op_result_payload(
    *,
    setup: CoinOpSetup,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    until_ready: bool,
    max_iterations: int,
    stop_reason: str,
    final_readiness: dict[str, int | bool | str] | None,
    operations: list[dict[str, object]],
) -> dict[str, object]:
    return {
        **scope_payload(setup.backend.scope),
        "coin_selection_mode": "explicit" if coin_ids else "adapter_auto_select",
        "denomination_target": denomination_target_payload(denomination_target),
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "denomination_readiness": final_readiness,
        "operations": operations,
        "signature_request_id": (
            str(operations[-1].get("signature_request_id", "")) if operations else ""
        ),
        "signature_state": (
            str(operations[-1].get("signature_state", "UNKNOWN")) if operations else "UNKNOWN"
        ),
        "waited": bool(operations[-1].get("waited", False)) if operations else False,
        "wait_events": (
            as_wait_events(operations[-1].get("wait_events", [])) if operations else []
        ),
        "fee_mojos": setup.fee_mojos,
        "fee_source": setup.fee_source,
    }


def resolve_venue_for_coin_prep(*, venue_override: str | None) -> str | None:
    if venue_override is None or not venue_override.strip():
        return None
    venue = venue_override.strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("coin-prep venue must be dexie or splash when provided")
    return venue


def resolve_market_denomination_entry(
    market: MarketConfig, *, size_base_units: int
) -> MarketLadderEntry:
    ladder = market.ladders.get("sell") or []
    if not ladder:
        raise ValueError(
            f"market {market.market_id} has no sell ladder; cannot resolve denomination target"
        )
    for entry in ladder:
        if int(entry.size_base_units) == int(size_base_units):
            return entry
    allowed = ", ".join(str(int(row.size_base_units)) for row in ladder)
    raise ValueError(
        f"size_base_units not configured for market sell ladder; use one of: {allowed}"
    )


@dataclass(slots=True)
class CoinOpSetup:
    program: ProgramConfig
    market: MarketConfig
    backend: CoinOpBackend
    resolved_asset_id: str
    fee_mojos: int
    fee_source: str
    selected_venue: str | None

    @property
    def wallet(self) -> CloudWalletAdapter:
        if not isinstance(self.backend, CloudWalletCoinOpBackend):
            raise AttributeError("wallet is only available for cloud_wallet coin-op backend")
        return self.backend.wallet


@dataclass(slots=True)
class CoinOpSetupResult:
    setup: CoinOpSetup | None = None
    error_payload: dict[str, object] | None = None


def coin_op_setup(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    venue: str | None,
    canonical_asset_id_override: str | None = None,
    deps: CoinOpDeps = DEFAULT_COIN_OP_DEPS,
) -> CoinOpSetupResult:
    program = load_program_config(program_path)
    selected_venue = resolve_venue_for_coin_prep(venue_override=venue)
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
    canonical = canonical_asset_id_override or str(market.base_asset)
    hint = canonical_asset_id_override or str(market.base_symbol)
    try:
        if canonical_asset_id_override:
            from greenfloor.config.models import coin_ops_execution_backend

            if coin_ops_execution_backend(program) == "signer":
                resolved_asset_id = resolve_signer_asset_id(
                    program, canonical_asset_id=canonical, symbol_hint=hint
                )
            else:
                wallet = deps.new_cloud_wallet_adapter(program)
                resolved_asset_id = deps.resolve_cloud_wallet_asset_id(
                    wallet=wallet,
                    canonical_asset_id=canonical,
                    symbol_hint=hint,
                    program_home_dir=str(program.home_dir),
                )
        else:
            resolved_asset_id = resolve_coin_op_base_asset_id(
                program=program, market=market, deps=deps
            )
        backend = build_coin_op_backend(
            program=program,
            market=market,
            selected_venue=selected_venue,
            resolved_asset_id=resolved_asset_id,
            deps=deps,
        )
    except ValueError as exc:
        return CoinOpSetupResult(
            error_payload={
                "error": str(exc),
                "market_id": market.market_id,
            }
        )
    if isinstance(backend, CloudWalletCoinOpBackend):
        fee_result = resolve_coin_op_fee(
            network=network,
            minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
            market=market,
            selected_venue=selected_venue,
            wallet=backend.wallet,
            deps=deps,
        )
        if not fee_result.ok:
            return CoinOpSetupResult(error_payload=fee_result.error_payload)
        fee_mojos = fee_result.fee_mojos
        fee_source = fee_result.fee_source
    else:
        fee_mojos = 0
        fee_source = "signer_vault_no_fee"
    return CoinOpSetupResult(
        setup=CoinOpSetup(
            program=program,
            market=market,
            backend=backend,
            resolved_asset_id=resolved_asset_id,
            fee_mojos=fee_mojos,
            fee_source=fee_source,
            selected_venue=selected_venue,
        )
    )


@dataclass(slots=True)
class CoinOpIterationExecuteResult:
    signature_request_id: str
    initial_signature_state: str
    readiness_kwargs: dict[str, int]


@dataclass(slots=True)
class CoinOpIterationEarlyExit:
    return_code: int
    unresolved_coin_ids: list[str] | None = None
    stop_reason: str = ""
    error_payload: dict[str, object] | None = None


@dataclass(slots=True)
class CoinOpIterationSkipLoop:
    stop_reason: str


@dataclass(slots=True)
class CoinOpIterationNeedsConfirmation:
    message: str
    override: Literal["force_split_when_ready", "allow_lock_all_spendable"]
    decline_step: CoinOpIterationEarlyExit | CoinOpIterationSkipLoop


CoinOpIterationStep = (
    CoinOpIterationExecuteResult
    | CoinOpIterationEarlyExit
    | CoinOpIterationSkipLoop
    | CoinOpIterationNeedsConfirmation
)


@dataclass(slots=True)
class CoinOpStepOutcome:
    step: CoinOpIterationStep
    split_gate: dict[str, int | bool | str] | None = None


@dataclass(slots=True)
class CoinOpLoopResult:
    operations: list[dict[str, object]]
    final_readiness: dict[str, int | bool | str] | None
    stop_reason: str
    unresolved_coin_ids: list[str]
    split_gate: dict[str, int | bool | str] | None = None
    early_return_code: int | None = None
    error_payload: dict[str, object] | None = None


def run_coin_op_iteration_loop(
    *,
    setup: CoinOpSetup,
    network: str,
    no_wait: bool,
    until_ready: bool,
    max_iterations: int,
    coin_ids: list[str],
    denomination_target: DenominationTarget,
    readiness_asset_id: str,
    run_step: Callable[[int, list[dict[str, Any]], set[str]], CoinOpStepOutcome],
) -> CoinOpLoopResult:
    backend = setup.backend
    if isinstance(backend, SignerCoinOpBackend):
        backend.no_wait = no_wait
    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    split_gate: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        wallet_coins = backend.list_wallet_coins()
        existing_coin_ids = {
            str(c.get("id", c.get("name", ""))).strip()
            for c in wallet_coins
            if str(c.get("id", c.get("name", ""))).strip()
        }
        outcome = run_step(iteration, wallet_coins, existing_coin_ids)
        if outcome.split_gate is not None:
            split_gate = outcome.split_gate
        step = outcome.step

        if isinstance(step, CoinOpIterationEarlyExit):
            return CoinOpLoopResult(
                operations=operations,
                final_readiness=final_readiness,
                stop_reason=step.stop_reason or stop_reason,
                unresolved_coin_ids=list(step.unresolved_coin_ids or []),
                split_gate=split_gate,
                early_return_code=step.return_code,
                error_payload=step.error_payload,
            )
        if isinstance(step, CoinOpIterationSkipLoop):
            stop_reason = step.stop_reason
            break
        if isinstance(step, CoinOpIterationNeedsConfirmation):
            raise RuntimeError(
                "coin_op_iteration_needs_confirmation must be handled before the loop"
            )

        iteration_payload, final_readiness = backend.build_iteration_payload(
            operation_id=step.signature_request_id,
            operation_state=step.initial_signature_state,
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            readiness_asset_id=readiness_asset_id,
            readiness_kwargs=step.readiness_kwargs,
            denomination_target=denomination_target,
        )
        operations.append(iteration_payload)

        should_break, reason = coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    return CoinOpLoopResult(
        operations=operations,
        final_readiness=final_readiness,
        stop_reason=stop_reason,
        unresolved_coin_ids=unresolved_coin_ids,
        split_gate=split_gate,
    )
