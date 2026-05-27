"""Single runtime composition root for offer execution orchestration."""

from __future__ import annotations

from greenfloor.runtime.cloud_wallet.assets import (
    resolve_cloud_wallet_offer_asset_ids,
    seed_cloud_wallet_assets_cache,
)
from greenfloor.runtime.cloud_wallet.build_post import build_and_post_offer_cloud_wallet
from greenfloor.runtime.cloud_wallet.deps import (
    CloudWalletOfferDeps,
    default_cloud_wallet_offer_deps,
)
from greenfloor.runtime.offer_orchestration import OfferPostDeps, default_offer_post_deps
from greenfloor.runtime.offer_publish import (
    is_transient_dexie_visibility_404_error,
    verify_offer_text_for_dexie,
    verify_offer_visible_on_dexie,
)
from greenfloor.runtime.offer_runtime import (
    SignerOfferDeps,
    build_and_post_offer_signer,
    default_signer_offer_deps,
)

__all__ = [
    "CloudWalletOfferDeps",
    "OfferPostDeps",
    "SignerOfferDeps",
    "build_and_post_offer_cloud_wallet",
    "build_and_post_offer_signer",
    "default_cloud_wallet_offer_deps",
    "default_offer_post_deps",
    "default_signer_offer_deps",
    "is_transient_dexie_visibility_404_error",
    "resolve_cloud_wallet_offer_asset_ids",
    "seed_cloud_wallet_assets_cache",
    "verify_offer_text_for_dexie",
    "verify_offer_visible_on_dexie",
]
