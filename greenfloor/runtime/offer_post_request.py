"""Shared offer post request model and backend dispatch."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from greenfloor.config.models import OfferExecutionBackend
from greenfloor.runtime.local_offer import make_local_offer_create_fn
from greenfloor.runtime.offer_build_context import OfferBuildContext
from greenfloor.runtime.offer_orchestration import (
    OfferPostDeps,
    build_and_post_offer,
)
from greenfloor.runtime.offer_runtime import SignerOfferDeps, build_and_post_offer_signer


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

    @classmethod
    def from_mapping(cls, data: dict[str, Any]) -> ManagedOfferPostResult:
        offer_id_raw = data.get("offer_id")
        clean_offer_id = str(offer_id_raw).strip() if offer_id_raw not in (None, "") else None
        return cls(
            success=bool(data.get("success", False)),
            error=str(data.get("error", "")).strip(),
            offer_id=clean_offer_id,
            offer_create_ms=_optional_int(data.get("offer_create_ms")),
            offer_publish_ms=_optional_int(data.get("offer_publish_ms")),
            offer_total_ms=_optional_int(data.get("offer_total_ms")),
            offer_create_phase_ms=_optional_int(data.get("offer_create_phase_ms")),
            offer_artifact_wait_ms=_optional_int(data.get("offer_artifact_wait_ms")),
        )


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

    def _managed_backend_kwargs(
        self,
        *,
        deps: SignerOfferDeps | None = None,
        emit_output: bool = True,
        persist_results: bool = True,
    ) -> dict[str, Any]:
        return {
            "build_ctx": self.build_ctx,
            "size_base_units": self.size_base_units,
            "repeat": self.repeat,
            "publish_venue": self.publish_venue,
            "dexie_base_url": self.dexie_base_url,
            "splash_base_url": self.splash_base_url,
            "drop_only": self.drop_only,
            "claim_rewards": self.claim_rewards,
            "dry_run": self.dry_run,
            "deps": deps,
            "emit_output": emit_output,
            "persist_results": persist_results,
        }

    def run_signer(
        self,
        *,
        deps: SignerOfferDeps | None = None,
        emit_output: bool = True,
        persist_results: bool = True,
    ) -> tuple[int, dict[str, Any]]:
        return build_and_post_offer_signer(
            **self._managed_backend_kwargs(
                deps=deps,
                emit_output=emit_output,
                persist_results=persist_results,
            ),
        )

    def run_local_bls(
        self,
        *,
        capture_dir_path: Path | None,
        build_offer_fn: collections.abc.Callable[[dict[str, Any]], str],
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
                build_offer_fn=build_offer_fn,
            ),
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
        build_offer_fn: collections.abc.Callable[[dict[str, Any]], str],
        post_deps: OfferPostDeps,
        path_extra_fields: dict[str, Any] | None = None,
    ) -> int:
        if backend == "signer":
            exit_code, _ = self.run_signer()
        else:
            exit_code, _ = self.run_local_bls(
                capture_dir_path=capture_dir_path,
                build_offer_fn=build_offer_fn,
                post_deps=post_deps,
                path_extra_fields=path_extra_fields,
            )
        return exit_code

    def run_managed(
        self,
        backend: OfferExecutionBackend,
    ) -> tuple[int, dict[str, Any]]:
        if backend == "bls":
            raise ValueError("managed offer post does not support bls backend")
        if backend != "signer":
            raise ValueError("managed offer post requires signer backend")
        return self.run_signer(emit_output=False, persist_results=False)
