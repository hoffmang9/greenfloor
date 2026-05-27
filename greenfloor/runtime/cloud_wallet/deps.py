"""Injectable dependencies for Cloud Wallet offer build-and-post."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.runtime.cloud_wallet.adapter import format_json_output, new_cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet.assets import (
    recent_market_resolved_asset_id_hints,
    resolve_cloud_wallet_offer_asset_ids,
)
from greenfloor.runtime.cloud_wallet.bootstrap import (
    configured_ensure_offer_bootstrap_denominations,
)
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.offer_orchestration import OfferPostDeps, default_offer_post_deps


@dataclass(frozen=True, slots=True)
class CloudWalletOfferDeps:
    wallet_factory: collections.abc.Callable[[Any], CloudWalletAdapter]
    post_deps: OfferPostDeps
    recent_market_resolved_asset_id_hints_fn: collections.abc.Callable[
        ..., tuple[str | None, str | None]
    ]
    resolve_cloud_wallet_offer_asset_ids_fn: collections.abc.Callable[..., tuple[str, str]]
    ensure_offer_bootstrap_denominations_fn: collections.abc.Callable[..., dict[str, Any]]
    cloud_wallet_create_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]]
    cloud_wallet_wait_offer_artifact_phase_fn: collections.abc.Callable[..., str]


def default_cloud_wallet_offer_deps() -> CloudWalletOfferDeps:
    return CloudWalletOfferDeps(
        wallet_factory=new_cloud_wallet_adapter,
        post_deps=default_offer_post_deps(format_output_fn=format_json_output),
        recent_market_resolved_asset_id_hints_fn=recent_market_resolved_asset_id_hints,
        resolve_cloud_wallet_offer_asset_ids_fn=resolve_cloud_wallet_offer_asset_ids,
        ensure_offer_bootstrap_denominations_fn=configured_ensure_offer_bootstrap_denominations,
        cloud_wallet_create_offer_phase_fn=cloud_wallet_create_offer_phase,
        cloud_wallet_wait_offer_artifact_phase_fn=cloud_wallet_wait_offer_artifact_phase,
    )
