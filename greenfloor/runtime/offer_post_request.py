"""Shared offer post request model and backend dispatch."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.config.models import OfferExecutionBackend
from greenfloor.runtime.cloud_wallet.build_post import build_and_post_offer_cloud_wallet
from greenfloor.runtime.cloud_wallet.deps import CloudWalletOfferDeps
from greenfloor.runtime.local_offer import make_local_offer_create_fn
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import (
    BootstrapPolicy,
    OfferPostDeps,
    build_and_post_offer,
)
from greenfloor.runtime.offer_runtime import SignerOfferDeps, build_and_post_offer_signer


def parse_managed_offer_post_result(
    exit_code: int,
    payload: dict[str, Any],
) -> dict[str, Any]:
    results = payload.get("results", [])
    result = (
        results[0].get("result", {})
        if isinstance(results, list) and results and isinstance(results[0], dict)
        else {}
    )
    timing_payload = result.get("timing_ms", {}) if isinstance(result, dict) else {}

    def _opt_int(key: str) -> int | None:
        value = timing_payload.get(key) if isinstance(timing_payload, dict) else None
        return int(value) if value is not None else None

    timing_fields = {
        "offer_create_ms": _opt_int("create_total_ms"),
        "offer_publish_ms": _opt_int("publish_ms"),
        "offer_total_ms": _opt_int("total_ms"),
        "offer_create_phase_ms": _opt_int("create_phase_ms"),
        "offer_artifact_wait_ms": _opt_int("artifact_wait_ms"),
    }
    if exit_code != 0:
        error = str(result.get("error", "")).strip() if isinstance(result, dict) else ""
        return {
            "success": False,
            "error": error or f"managed_offer_post_exit_code:{exit_code}",
            **timing_fields,
        }
    if not isinstance(results, list) or not results:
        return {"success": False, "error": "managed_offer_post_missing_results"}
    result = results[0].get("result", {}) if isinstance(results[0], dict) else {}
    if not isinstance(result, dict):
        result = {}
    success = bool(result.get("success", False)) and int(payload.get("publish_failures", 1)) == 0
    return {
        "success": success,
        "offer_id": str(result.get("id", "")).strip() or None,
        "error": str(result.get("error", "")).strip() if not success else "",
        **timing_fields,
    }


@dataclass(frozen=True, slots=True)
class OfferPostRequest:
    build_ctx: OfferBuildContext
    size_base_units: int
    repeat: int
    publish_venue: str
    dexie_base_url: str
    splash_base_url: str
    drop_only: bool
    claim_rewards: bool
    dry_run: bool

    def run_signer(
        self,
        *,
        deps: SignerOfferDeps | None = None,
        emit_output: bool = True,
        persist_results: bool = True,
    ) -> tuple[int, dict[str, Any]]:
        return build_and_post_offer_signer(
            program=self.build_ctx.program,
            market=self.build_ctx.market,
            size_base_units=self.size_base_units,
            repeat=self.repeat,
            publish_venue=self.publish_venue,
            dexie_base_url=self.dexie_base_url,
            splash_base_url=self.splash_base_url,
            drop_only=self.drop_only,
            claim_rewards=self.claim_rewards,
            dry_run=self.dry_run,
            build_ctx=self.build_ctx,
            deps=deps,
            emit_output=emit_output,
            persist_results=persist_results,
        )

    def run_cloud_wallet(
        self,
        *,
        deps: CloudWalletOfferDeps | None = None,
        offer_artifact_timeout_seconds: int | None = None,
        emit_output: bool = True,
        persist_results: bool = True,
    ) -> tuple[int, dict[str, Any]]:
        return build_and_post_offer_cloud_wallet(
            program=self.build_ctx.program,
            market=self.build_ctx.market,
            size_base_units=self.size_base_units,
            repeat=self.repeat,
            publish_venue=self.publish_venue,
            dexie_base_url=self.dexie_base_url,
            splash_base_url=self.splash_base_url,
            drop_only=self.drop_only,
            claim_rewards=self.claim_rewards,
            dry_run=self.dry_run,
            build_ctx=self.build_ctx,
            deps=deps,
            emit_output=emit_output,
            persist_results=persist_results,
            offer_artifact_timeout_seconds=offer_artifact_timeout_seconds,
        )

    def run_local_bls(
        self,
        *,
        capture_dir_path: Path | None,
        build_offer_text_fn: collections.abc.Callable[[dict[str, Any]], str],
        post_deps: OfferPostDeps,
        path_label: str = "local",
        path_extra_fields: dict[str, Any] | None = None,
        persist_results: bool = True,
        emit_output: bool = True,
    ) -> tuple[int, dict[str, Any]]:
        return build_and_post_offer(
            build_ctx=self.build_ctx,
            size_base_units=self.size_base_units,
            repeat=self.repeat,
            publish_venue=self.publish_venue,
            dexie_base_url=self.dexie_base_url,
            splash_base_url=self.splash_base_url,
            drop_only=self.drop_only,
            claim_rewards=self.claim_rewards,
            dry_run=self.dry_run,
            resolved_base_asset_id=str(self.build_ctx.market.base_asset),
            resolved_quote_asset_id=self.build_ctx.resolved_quote_asset,
            bootstrap_phase_fn=None,
            create_offer_fn=make_local_offer_create_fn(
                self.build_ctx,
                dry_run=self.dry_run,
                capture_dir_path=capture_dir_path,
                build_offer_text_fn=build_offer_text_fn,
            ),
            bootstrap_policy=BootstrapPolicy(allow_split_fallback=False),
            path_label=path_label,
            path_extra_fields=path_extra_fields,
            post_deps=post_deps,
            emit_output=emit_output,
            persist_results=persist_results,
        )

    def run_cli(
        self,
        backend: OfferExecutionBackend,
        *,
        capture_dir_path: Path | None,
        build_offer_text_fn: collections.abc.Callable[[dict[str, Any]], str],
        post_deps: OfferPostDeps,
        path_extra_fields: dict[str, Any] | None = None,
    ) -> int:
        if backend == "signer":
            exit_code, _ = self.run_signer()
        elif backend == "cloud_wallet":
            exit_code, _ = self.run_cloud_wallet()
        else:
            exit_code, _ = self.run_local_bls(
                capture_dir_path=capture_dir_path,
                build_offer_text_fn=build_offer_text_fn,
                post_deps=post_deps,
                path_extra_fields=path_extra_fields,
            )
        return exit_code

    def run_managed(
        self,
        backend: OfferExecutionBackend,
        *,
        offer_artifact_timeout_seconds: int | None = None,
    ) -> tuple[int, dict[str, Any]]:
        if backend == "bls":
            raise ValueError("managed offer post does not support bls backend")
        if backend == "signer":
            return self.run_signer(emit_output=False, persist_results=False)
        return self.run_cloud_wallet(
            offer_artifact_timeout_seconds=offer_artifact_timeout_seconds,
            emit_output=False,
            persist_results=False,
        )
