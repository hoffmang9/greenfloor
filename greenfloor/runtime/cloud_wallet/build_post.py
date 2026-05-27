"""Cloud Wallet build-and-post orchestration entry."""

from __future__ import annotations

import time
from typing import Any

from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.cloud_wallet.deps import (
    CloudWalletOfferDeps,
    default_cloud_wallet_offer_deps,
)
from greenfloor.runtime.offer_orchestration import (
    BootstrapPolicy,
    OfferCreateFailure,
    OfferCreateOutcome,
    build_and_post_offer,
)


def build_and_post_offer_cloud_wallet(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    size_base_units: int,
    repeat: int,
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    quote_price: float,
    dry_run: bool,
    action_side: str = "sell",
    offer_artifact_timeout_seconds: int | None = None,
    emit_output: bool = True,
    persist_results: bool = True,
    deps: CloudWalletOfferDeps | None = None,
) -> tuple[int, dict[str, Any]]:
    resolved_deps = deps or default_cloud_wallet_offer_deps()
    resolved_artifact_timeout_seconds = (
        int(getattr(program, "runtime_cloud_wallet_offer_artifact_timeout_seconds", 30))
        if offer_artifact_timeout_seconds is None
        else int(offer_artifact_timeout_seconds)
    )
    wallet = resolved_deps.wallet_factory(program)
    cfg_base_global = str(getattr(market, "cloud_wallet_base_global_id", "")).strip()
    cfg_quote_global = str(getattr(market, "cloud_wallet_quote_global_id", "")).strip()
    db_base_hint, db_quote_hint = resolved_deps.recent_market_resolved_asset_id_hints_fn(
        program_home_dir=str(program.home_dir),
        market_id=str(market.market_id),
    )
    resolved_base_asset_id, resolved_quote_asset_id = (
        resolved_deps.resolve_cloud_wallet_offer_asset_ids_fn(
            wallet=wallet,
            base_asset_id=str(market.base_asset),
            quote_asset_id=str(market.quote_asset),
            base_symbol_hint=str(getattr(market, "base_symbol", "") or ""),
            quote_symbol_hint=str(getattr(market, "quote_asset", "") or ""),
            base_global_id_hint=cfg_base_global or db_base_hint,
            quote_global_id_hint=cfg_quote_global or db_quote_hint,
            program_home_dir=str(program.home_dir),
        )
    )
    expiry_unit, expiry_value = resolved_deps.resolve_offer_expiry_for_market_fn(market)
    offer_fee_mojos, _ = resolved_deps.post_deps.resolve_maker_offer_fee_fn(
        network=program.app_network
    )
    bootstrap_signature_wait_timeout_seconds = int(
        program.runtime_offer_bootstrap_signature_wait_timeout_seconds
    )
    bootstrap_signature_warning_interval_seconds = int(
        program.runtime_offer_bootstrap_signature_warning_interval_seconds
    )
    bootstrap_wait_timeout_seconds = int(program.runtime_offer_bootstrap_wait_timeout_seconds)
    bootstrap_wait_mempool_warning_seconds = int(
        program.runtime_offer_bootstrap_wait_mempool_warning_seconds
    )
    bootstrap_wait_confirmation_warning_seconds = int(
        program.runtime_offer_bootstrap_wait_confirmation_warning_seconds
    )
    create_signature_wait_timeout_seconds = int(
        program.runtime_cloud_wallet_create_signature_wait_timeout_seconds
    )
    create_signature_warning_interval_seconds = int(
        program.runtime_cloud_wallet_create_signature_warning_interval_seconds
    )

    def bootstrap(**kwargs: Any) -> dict[str, Any]:
        return resolved_deps.ensure_offer_bootstrap_denominations_fn(
            wallet=wallet,
            bootstrap_signature_wait_timeout_seconds=bootstrap_signature_wait_timeout_seconds,
            bootstrap_signature_warning_interval_seconds=bootstrap_signature_warning_interval_seconds,
            bootstrap_wait_timeout_seconds=bootstrap_wait_timeout_seconds,
            bootstrap_wait_mempool_warning_seconds=bootstrap_wait_mempool_warning_seconds,
            bootstrap_wait_confirmation_warning_seconds=bootstrap_wait_confirmation_warning_seconds,
            **kwargs,
        )

    def create(**kwargs: Any) -> OfferCreateOutcome:
        create_started = time.monotonic()
        create_phase = resolved_deps.cloud_wallet_create_offer_phase_fn(
            wallet=wallet,
            market=kwargs["market"],
            size_base_units=kwargs["size_base_units"],
            quote_price=kwargs["quote_price"],
            resolved_base_asset_id=kwargs["resolved_base_asset_id"],
            resolved_quote_asset_id=kwargs["resolved_quote_asset_id"],
            offer_fee_mojos=offer_fee_mojos,
            split_input_coins_fee=0,
            expiry_unit=expiry_unit,
            expiry_value=expiry_value,
            action_side=kwargs["action_side"],
            signature_wait_timeout_seconds=create_signature_wait_timeout_seconds,
            signature_wait_warning_interval_seconds=create_signature_warning_interval_seconds,
        )
        create_phase_ms = int((time.monotonic() - create_started) * 1000)
        wait_started = time.monotonic()
        try:
            offer_text = resolved_deps.cloud_wallet_wait_offer_artifact_phase_fn(
                wallet=wallet,
                known_markers=set(create_phase["known_offer_markers"]),
                offer_request_started_at=create_phase["offer_request_started_at"],
                signature_request_id=str(create_phase["signature_request_id"]).strip(),
                timeout_seconds=resolved_artifact_timeout_seconds,
            )
        except RuntimeError as exc:
            artifact_wait_ms = int((time.monotonic() - wait_started) * 1000)
            raise OfferCreateFailure(
                str(exc),
                create_phase_ms=create_phase_ms,
                artifact_wait_ms=artifact_wait_ms,
                create_total_ms=int((time.monotonic() - create_started) * 1000),
                extra={
                    "signature_request_id": str(create_phase["signature_request_id"]).strip(),
                    "signature_state": str(create_phase["signature_state"]).strip(),
                    "wait_events": list(create_phase["wait_events"]),
                },
            ) from exc
        artifact_wait_ms = int((time.monotonic() - wait_started) * 1000)
        return OfferCreateOutcome(
            offer_text=str(offer_text).strip(),
            expires_at=str(create_phase["expires_at"]),
            side=str(create_phase.get("side", kwargs["action_side"])),
            create_phase_ms=create_phase_ms,
            artifact_wait_ms=artifact_wait_ms,
            create_total_ms=int((time.monotonic() - create_started) * 1000),
            extra={
                "signature_request_id": str(create_phase["signature_request_id"]).strip(),
                "signature_state": str(create_phase["signature_state"]).strip(),
                "wait_events": list(create_phase["wait_events"]),
            },
        )

    return build_and_post_offer(
        program=program,
        market=market,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        quote_price=quote_price,
        dry_run=dry_run,
        action_side=action_side,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        bootstrap_phase_fn=bootstrap,
        create_offer_fn=create,
        bootstrap_policy=BootstrapPolicy(allow_split_fallback=True),
        path_label="cloud_wallet",
        post_deps=resolved_deps.post_deps,
        emit_output=emit_output,
        persist_results=persist_results,
    )
