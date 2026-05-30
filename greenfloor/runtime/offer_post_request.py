"""Shared offer post request model and signer backend dispatch."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.runtime.daemon_config_paths import resolve_daemon_config_paths
from greenfloor.runtime.engine_build_and_post import run_build_and_post_offer_in_process
from greenfloor.runtime.offer_build_context import OfferBuildContext


@dataclass(frozen=True, slots=True)
class ManagedOfferPostResult:
    success: bool
    error: str = ""
    offer_id: str | None = None
    offer_create_ms: int | None = None
    offer_publish_ms: int | None = None
    offer_total_ms: int | None = None
    offer_create_phase_ms: int | None = None
    offer_artifact_wait_ms: int | None = None

    def timing_extra(self) -> dict[str, int]:
        extra: dict[str, int] = {}
        for key in (
            "offer_create_ms",
            "offer_publish_ms",
            "offer_total_ms",
            "offer_create_phase_ms",
            "offer_artifact_wait_ms",
        ):
            value = getattr(self, key)
            if value is not None:
                extra[key] = int(value)
        return extra


def _optional_int(value: Any) -> int | None:
    return int(value) if value is not None else None


def parse_managed_offer_post_result(
    exit_code: int,
    payload: dict[str, Any],
) -> ManagedOfferPostResult:
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
        return ManagedOfferPostResult(
            success=False,
            error=error or f"managed_offer_post_exit_code:{exit_code}",
            offer_create_ms=timing_fields["offer_create_ms"],
            offer_publish_ms=timing_fields["offer_publish_ms"],
            offer_total_ms=timing_fields["offer_total_ms"],
            offer_create_phase_ms=timing_fields["offer_create_phase_ms"],
            offer_artifact_wait_ms=timing_fields["offer_artifact_wait_ms"],
        )
    if not isinstance(results, list) or not results:
        return ManagedOfferPostResult(
            success=False,
            error="managed_offer_post_missing_results",
        )
    result = results[0].get("result", {}) if isinstance(results[0], dict) else {}
    if not isinstance(result, dict):
        result = {}
    success = bool(result.get("success", False)) and int(payload.get("publish_failures", 1)) == 0
    offer_id_raw = str(result.get("id", "")).strip()
    return ManagedOfferPostResult(
        success=success,
        offer_id=offer_id_raw or None,
        error=str(result.get("error", "")).strip() if not success else "",
        offer_create_ms=timing_fields["offer_create_ms"],
        offer_publish_ms=timing_fields["offer_publish_ms"],
        offer_total_ms=timing_fields["offer_total_ms"],
        offer_create_phase_ms=timing_fields["offer_create_phase_ms"],
        offer_artifact_wait_ms=timing_fields["offer_artifact_wait_ms"],
    )


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
        emit_output: bool = True,
        persist_results: bool = True,
    ) -> tuple[int, dict[str, Any]]:
        del emit_output
        paths = resolve_daemon_config_paths(
            self.build_ctx.program,
            self.build_ctx.program_path,
        )
        market = self.build_ctx.market
        return run_build_and_post_offer_in_process(
            paths=paths,
            network=self.build_ctx.network,
            market_id=str(market.market_id),
            size_base_units=self.size_base_units,
            repeat=self.repeat,
            publish_venue=self.publish_venue,
            dexie_base_url=self.dexie_base_url,
            splash_base_url=self.splash_base_url,
            drop_only=self.drop_only,
            claim_rewards=self.claim_rewards,
            dry_run=self.dry_run,
            action_side=self.build_ctx.action_side,
            persist_results=persist_results,
        )

    def run_cli(
        self,
    ) -> int:
        exit_code, _ = self.run_signer()
        return exit_code

    def run_managed(
        self,
    ) -> tuple[int, dict[str, Any]]:
        return self.run_signer(persist_results=False)
