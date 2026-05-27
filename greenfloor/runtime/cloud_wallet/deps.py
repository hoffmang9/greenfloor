"""Injectable dependencies for Cloud Wallet offer build-and-post."""

from __future__ import annotations

import collections.abc
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.runtime.cloud_wallet.adapter import new_cloud_wallet_adapter
from greenfloor.runtime.cloud_wallet.assets import (
    recent_market_resolved_asset_id_hints,
    resolve_cloud_wallet_offer_asset_ids,
)
from greenfloor.runtime.cloud_wallet.bootstrap import ensure_offer_bootstrap_denominations
from greenfloor.runtime.cloud_wallet.phases import (
    cloud_wallet_create_offer_phase,
    cloud_wallet_wait_offer_artifact_phase,
)
from greenfloor.runtime.coinset_runtime import resolve_maker_offer_fee
from greenfloor.runtime.offer_publish import (
    dexie_offer_view_url,
    initialize_manager_file_logging,
    log_signed_offer_artifact,
    post_offer_phase,
    resolve_offer_expiry_for_market,
    verify_offer_text_for_dexie,
)


@dataclass(frozen=True, slots=True)
class CloudWalletOfferDeps:
    wallet_factory: collections.abc.Callable[[Any], CloudWalletAdapter]
    dexie_adapter_cls: type[DexieAdapter]
    splash_adapter_cls: type[SplashAdapter]
    initialize_manager_file_logging_fn: collections.abc.Callable[..., None]
    recent_market_resolved_asset_id_hints_fn: collections.abc.Callable[
        ..., tuple[str | None, str | None]
    ]
    resolve_cloud_wallet_offer_asset_ids_fn: collections.abc.Callable[..., tuple[str, str]]
    resolve_maker_offer_fee_fn: collections.abc.Callable[..., tuple[int, str]]
    resolve_offer_expiry_for_market_fn: collections.abc.Callable[..., tuple[str, int]]
    ensure_offer_bootstrap_denominations_fn: collections.abc.Callable[..., dict[str, Any]]
    cloud_wallet_create_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]]
    cloud_wallet_wait_offer_artifact_phase_fn: collections.abc.Callable[..., str]
    log_signed_offer_artifact_fn: collections.abc.Callable[..., None]
    verify_offer_text_for_dexie_fn: collections.abc.Callable[[str], str | None]
    post_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]]
    dexie_offer_view_url_fn: collections.abc.Callable[..., str]


def default_cloud_wallet_offer_deps() -> CloudWalletOfferDeps:
    return CloudWalletOfferDeps(
        wallet_factory=new_cloud_wallet_adapter,
        dexie_adapter_cls=DexieAdapter,
        splash_adapter_cls=SplashAdapter,
        initialize_manager_file_logging_fn=initialize_manager_file_logging,
        recent_market_resolved_asset_id_hints_fn=recent_market_resolved_asset_id_hints,
        resolve_cloud_wallet_offer_asset_ids_fn=resolve_cloud_wallet_offer_asset_ids,
        resolve_maker_offer_fee_fn=resolve_maker_offer_fee,
        resolve_offer_expiry_for_market_fn=resolve_offer_expiry_for_market,
        ensure_offer_bootstrap_denominations_fn=ensure_offer_bootstrap_denominations,
        cloud_wallet_create_offer_phase_fn=cloud_wallet_create_offer_phase,
        cloud_wallet_wait_offer_artifact_phase_fn=cloud_wallet_wait_offer_artifact_phase,
        log_signed_offer_artifact_fn=log_signed_offer_artifact,
        verify_offer_text_for_dexie_fn=verify_offer_text_for_dexie,
        post_offer_phase_fn=post_offer_phase,
        dexie_offer_view_url_fn=dexie_offer_view_url,
    )
