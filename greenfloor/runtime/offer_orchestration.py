"""Shared bootstrap → create → verify → publish offer orchestration."""

from __future__ import annotations

import collections.abc
import json
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.core import offer_policy
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.offer_bootstrap import BootstrapPhaseResult
from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_publish import (
    dexie_offer_view_url,
    log_signed_offer_artifact,
    post_offer_phase,
)
from greenfloor.storage.sqlite import SqliteStore


@dataclass(frozen=True, slots=True)
class OfferCreateOutcome:
    offer_text: str
    expires_at: str
    side: str
    create_phase_ms: int | None = None
    artifact_wait_ms: int | None = None
    create_total_ms: int | None = None
    extra: dict[str, Any] = field(default_factory=dict)


class OfferCreateFailure(Exception):
    """Create-phase failure with structured fields for offer result payloads."""

    def __init__(
        self,
        message: str,
        *,
        create_phase_ms: int | None = None,
        artifact_wait_ms: int | None = None,
        create_total_ms: int | None = None,
        extra: dict[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.create_phase_ms = create_phase_ms
        self.artifact_wait_ms = artifact_wait_ms
        self.create_total_ms = create_total_ms
        self.extra = dict(extra or {})


def _offer_policy_error(exc: Exception) -> str:
    return f"offer_policy_error:{exc}"


def bootstrap_blocks_offer(bootstrap_result: BootstrapPhaseResult) -> tuple[bool, str | None]:
    error = offer_policy.bootstrap_block_error(
        bootstrap_status=bootstrap_result.status,
        bootstrap_reason=bootstrap_result.reason,
        bootstrap_ready=bootstrap_result.ready,
    )
    return (error is not None), error


def _iteration_timing_payload(
    *,
    started_ms: int,
    create_phase_ms: int | None,
    artifact_wait_ms: int | None,
    create_total_ms: int | None,
    publish_ms: int | None,
) -> dict[str, int | None]:
    now_ms = int(time.monotonic() * 1000)
    payload: dict[str, int | None] = {
        "create_phase_ms": create_phase_ms,
        "publish_ms": publish_ms,
        "total_ms": now_ms - started_ms,
    }
    if artifact_wait_ms is not None:
        payload["artifact_wait_ms"] = artifact_wait_ms
    if create_total_ms is not None:
        payload["create_total_ms"] = create_total_ms
    return payload


@dataclass(frozen=True, slots=True)
class OfferPostDeps:
    resolve_maker_offer_fee_fn: collections.abc.Callable[..., tuple[int, str]]
    log_signed_offer_artifact_fn: collections.abc.Callable[..., None]
    verify_offer_for_dexie_fn: collections.abc.Callable[[str], str | None]
    post_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]]
    dexie_offer_view_url_fn: collections.abc.Callable[..., str]
    dexie_adapter_cls: type[DexieAdapter]
    splash_adapter_cls: type[SplashAdapter]
    format_output_fn: collections.abc.Callable[[object], str]


def default_offer_post_deps(
    *,
    format_output_fn: collections.abc.Callable[[object], str] | None = None,
) -> OfferPostDeps:
    return OfferPostDeps(
        resolve_maker_offer_fee_fn=resolve_maker_offer_fee,
        log_signed_offer_artifact_fn=log_signed_offer_artifact,
        verify_offer_for_dexie_fn=offer_policy.verify_offer_for_dexie,
        post_offer_phase_fn=post_offer_phase,
        dexie_offer_view_url_fn=dexie_offer_view_url,
        dexie_adapter_cls=DexieAdapter,
        splash_adapter_cls=SplashAdapter,
        format_output_fn=format_output_fn or (lambda payload: json.dumps(payload, indent=2)),
    )


@dataclass(frozen=True, slots=True)
class OfferPostPersistRecord:
    offer_id: str
    market_id: str
    side: str
    size_base_units: int
    publish_venue: str
    resolved_base_asset_id: str
    resolved_quote_asset_id: str
    created_extra: dict[str, Any]


def persist_offer_post_records(
    store: SqliteStore,
    records: list[OfferPostPersistRecord],
) -> None:
    for record in records:
        store.upsert_offer_state(
            offer_id=record.offer_id,
            market_id=record.market_id,
            state=OfferLifecycleState.OPEN.value,
            last_seen_status=None,
        )
        audit_item: dict[str, Any] = {
            "size": int(record.size_base_units),
            "side": record.side,
            "status": "executed",
            "reason": f"{record.publish_venue}_post_success",
            "offer_id": record.offer_id,
            "attempts": 1,
        }
        audit_event: dict[str, Any] = {
            "market_id": record.market_id,
            "planned_count": 1,
            "executed_count": 1,
            "items": [audit_item],
            "venue": record.publish_venue,
            "resolved_base_asset_id": record.resolved_base_asset_id,
            "resolved_quote_asset_id": record.resolved_quote_asset_id,
        }
        audit_event.update(record.created_extra)
        store.add_audit_event(
            "strategy_offer_execution",
            audit_event,
            market_id=record.market_id,
        )


def _append_post_failure(
    post_results: list[dict[str, Any]],
    *,
    publish_venue: str,
    started_ms: int,
    error: str,
    create_phase_ms: int | None = None,
    artifact_wait_ms: int | None = None,
    create_total_ms: int | None = None,
    publish_ms: int | None = None,
    extra: dict[str, Any] | None = None,
    bootstrap: dict[str, Any] | None = None,
) -> None:
    result: dict[str, Any] = {
        "success": False,
        "error": error,
        "timing_ms": _iteration_timing_payload(
            started_ms=started_ms,
            create_phase_ms=create_phase_ms,
            artifact_wait_ms=artifact_wait_ms,
            create_total_ms=create_total_ms,
            publish_ms=publish_ms,
        ),
    }
    if bootstrap is not None:
        result["bootstrap"] = bootstrap
    if extra:
        result.update(extra)
    post_results.append({"venue": publish_venue, "result": result})


def execute_build_and_post_offer(
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
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    bootstrap_phase_fn: collections.abc.Callable[..., BootstrapPhaseResult] | None,
    create_offer_fn: collections.abc.Callable[..., OfferCreateOutcome],
    path_label: str,
    path_extra_fields: dict[str, Any] | None = None,
    post_deps: OfferPostDeps | None = None,
) -> tuple[int, dict[str, Any], list[OfferPostPersistRecord]]:
    resolved_post_deps = post_deps or default_offer_post_deps()
    program = build_ctx.program
    market = build_ctx.market
    quote_price = float(build_ctx.quote_price)
    side = build_ctx.action_side
    post_results: list[dict[str, Any]] = []
    built_offers_preview: list[dict[str, str]] = []
    bootstrap_actions: list[dict[str, Any]] = []
    persist_records: list[OfferPostPersistRecord] = []
    publish_failures = 0
    offer_fee_mojos, offer_fee_source = resolved_post_deps.resolve_maker_offer_fee_fn(
        network=program.app_network
    )
    dexie = (
        resolved_post_deps.dexie_adapter_cls(dexie_base_url)
        if (not dry_run and publish_venue == "dexie")
        else None
    )
    splash = (
        resolved_post_deps.splash_adapter_cls(splash_base_url)
        if (not dry_run and publish_venue == "splash")
        else None
    )

    for _ in range(repeat):
        started_ms = int(time.monotonic() * 1000)
        bootstrap_result = BootstrapPhaseResult(status="skipped", reason="dry_run")
        if dry_run:
            bootstrap_actions.append(bootstrap_result.to_manager_dict())
        elif bootstrap_phase_fn is None:
            bootstrap_result = BootstrapPhaseResult(
                status="skipped",
                reason="already_ready",
            )
            bootstrap_actions.append(bootstrap_result.to_manager_dict())
        else:
            bootstrap_result = bootstrap_phase_fn(
                program=program,
                market=market,
                resolved_base_asset_id=resolved_base_asset_id,
                resolved_quote_asset_id=resolved_quote_asset_id,
                quote_price=float(quote_price),
                action_side=side,
            )
            bootstrap_actions.append(bootstrap_result.to_manager_dict())
            blocked, error = bootstrap_blocks_offer(bootstrap_result)
            if blocked:
                _append_post_failure(
                    post_results,
                    publish_venue=publish_venue,
                    started_ms=started_ms,
                    error=str(error),
                    bootstrap=bootstrap_result.to_manager_dict(),
                )
                publish_failures += 1
                continue

        try:
            created = create_offer_fn(
                program=program,
                market=market,
                size_base_units=size_base_units,
                quote_price=quote_price,
                resolved_base_asset_id=resolved_base_asset_id,
                resolved_quote_asset_id=resolved_quote_asset_id,
                action_side=side,
            )
        except OfferCreateFailure as exc:
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=str(exc),
                create_phase_ms=exc.create_phase_ms,
                artifact_wait_ms=exc.artifact_wait_ms,
                create_total_ms=exc.create_total_ms,
                extra=exc.extra,
            )
            publish_failures += 1
            continue
        except (RuntimeError, TypeError, ValueError) as exc:
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=str(exc),
            )
            publish_failures += 1
            continue

        offer_text = str(created.offer_text).strip()
        if not offer_text:
            publish_failures += 1
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=f"{path_label}_offer_text_unavailable",
                create_phase_ms=created.create_phase_ms,
                artifact_wait_ms=created.artifact_wait_ms,
                create_total_ms=created.create_total_ms,
                extra=created.extra,
            )
            continue

        if dry_run:
            preview_item: dict[str, str] = {
                "offer_prefix": offer_text[:24],
                "offer_length": str(len(offer_text)),
            }
            dry_run_preview = created.extra.get("dry_run_preview")
            if isinstance(dry_run_preview, dict):
                preview_item.update(
                    {str(key): str(value) for key, value in dry_run_preview.items()}
                )
            built_offers_preview.append(preview_item)
            continue

        resolved_post_deps.log_signed_offer_artifact_fn(
            offer_text=offer_text,
            ticker=str(market.base_symbol),
            amount=int(size_base_units),
            trading_pair=f"{market.base_symbol}:{market.quote_asset}",
            expiry=str(created.expires_at),
        )
        try:
            verify_error = resolved_post_deps.verify_offer_for_dexie_fn(offer_text)
        except (RuntimeError, TypeError, ValueError) as exc:
            publish_failures += 1
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=_offer_policy_error(exc),
                create_phase_ms=created.create_phase_ms,
                artifact_wait_ms=created.artifact_wait_ms,
                create_total_ms=created.create_total_ms,
            )
            continue
        if verify_error:
            publish_failures += 1
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=verify_error,
                create_phase_ms=created.create_phase_ms,
                artifact_wait_ms=created.artifact_wait_ms,
                create_total_ms=created.create_total_ms,
            )
            continue

        publish_started = time.monotonic()
        try:
            asset_fields = offer_policy.expected_publish_asset_fields(
                side=created.side,
                base_symbol=str(market.base_symbol),
                quote_asset=str(market.quote_asset),
                resolved_base_asset_id=resolved_base_asset_id,
                resolved_quote_asset_id=resolved_quote_asset_id,
            )
        except (RuntimeError, TypeError, ValueError) as exc:
            publish_failures += 1
            _append_post_failure(
                post_results,
                publish_venue=publish_venue,
                started_ms=started_ms,
                error=_offer_policy_error(exc),
                create_phase_ms=created.create_phase_ms,
                artifact_wait_ms=created.artifact_wait_ms,
                create_total_ms=created.create_total_ms,
            )
            continue
        result = resolved_post_deps.post_offer_phase_fn(
            publish_venue=publish_venue,
            dexie=dexie,
            splash=splash,
            offer_text=offer_text,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
            **asset_fields,
        )
        publish_ms = int((time.monotonic() - publish_started) * 1000)
        if result.get("success") is False:
            publish_failures += 1
        offer_id = str(result.get("id", "")).strip()
        result_payload = {
            **result,
            **created.extra,
            "timing_ms": _iteration_timing_payload(
                started_ms=started_ms,
                create_phase_ms=created.create_phase_ms,
                artifact_wait_ms=created.artifact_wait_ms,
                create_total_ms=created.create_total_ms,
                publish_ms=publish_ms,
            ),
        }
        if publish_venue == "dexie" and offer_id:
            result_payload["offer_view_url"] = resolved_post_deps.dexie_offer_view_url_fn(
                dexie_base_url=dexie_base_url,
                offer_id=offer_id,
            )
        if offer_id and bool(result.get("success", False)):
            persist_records.append(
                OfferPostPersistRecord(
                    offer_id=offer_id,
                    market_id=str(market.market_id),
                    side=side,
                    size_base_units=int(size_base_units),
                    publish_venue=publish_venue,
                    resolved_base_asset_id=resolved_base_asset_id,
                    resolved_quote_asset_id=resolved_quote_asset_id,
                    created_extra=dict(created.extra),
                )
            )
        post_results.append({"venue": publish_venue, "result": result_payload})

    payload: dict[str, Any] = {
        "market_id": market.market_id,
        "pair": f"{market.base_asset}:{market.quote_asset}",
        "resolved_base_asset_id": resolved_base_asset_id,
        "resolved_quote_asset_id": resolved_quote_asset_id,
        "network": program.app_network,
        "size_base_units": size_base_units,
        "repeat": repeat,
        "publish_venue": publish_venue,
        "dexie_base_url": dexie_base_url,
        "splash_base_url": splash_base_url if publish_venue == "splash" else None,
        "drop_only": drop_only,
        "claim_rewards": claim_rewards,
        "dry_run": bool(dry_run),
        "publish_attempts": len(post_results),
        "publish_failures": publish_failures,
        "built_offers_preview": built_offers_preview,
        "bootstrap_actions": bootstrap_actions,
        "results": post_results,
        "offer_fee_mojos": offer_fee_mojos,
        "offer_fee_source": offer_fee_source,
        "execution_backend": path_label,
    }
    if path_extra_fields:
        payload.update(path_extra_fields)
    return (0 if publish_failures == 0 else 2), payload, persist_records


def build_and_post_offer(
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
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    bootstrap_phase_fn: collections.abc.Callable[..., BootstrapPhaseResult] | None,
    create_offer_fn: collections.abc.Callable[..., OfferCreateOutcome],
    path_label: str,
    path_extra_fields: dict[str, Any] | None = None,
    post_deps: OfferPostDeps | None = None,
    emit_output: bool = True,
    persist_results: bool = True,
) -> tuple[int, dict[str, Any]]:
    resolved_post_deps = post_deps or default_offer_post_deps()
    program = build_ctx.program
    exit_code, payload, persist_records = execute_build_and_post_offer(
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
        bootstrap_phase_fn=bootstrap_phase_fn,
        create_offer_fn=create_offer_fn,
        path_label=path_label,
        path_extra_fields=path_extra_fields,
        post_deps=resolved_post_deps,
    )
    if persist_results and persist_records:
        db_path = (Path(program.home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()
        store = SqliteStore(db_path)
        try:
            persist_offer_post_records(store, persist_records)
        finally:
            store.close()
    if emit_output:
        print(resolved_post_deps.format_output_fn(payload))
    return exit_code, payload
