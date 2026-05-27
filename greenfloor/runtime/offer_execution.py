"""Single runtime composition root for offer execution orchestration."""

from __future__ import annotations

from greenfloor.runtime.cloud_wallet.build_post import build_and_post_offer_cloud_wallet
from greenfloor.runtime.cloud_wallet.deps import (
    CloudWalletOfferDeps,
    default_cloud_wallet_offer_deps,
)
from greenfloor.runtime.local_offer import (
    LocalOfferBuildParams,
    local_offer_params_from_context,
    make_local_offer_create_fn,
)
from greenfloor.runtime.offer_build_context import OfferBuildContext, prepare_offer_build_context
from greenfloor.runtime.offer_orchestration import (
    BootstrapPolicy,
    OfferCreateFailure,
    OfferCreateOutcome,
    OfferPostDeps,
    OfferPostPersistRecord,
    build_and_post_offer,
    default_offer_post_deps,
    execute_build_and_post_offer,
    persist_offer_post_records,
)
from greenfloor.runtime.offer_runtime import (
    SignerOfferDeps,
    build_and_post_offer_signer,
    default_signer_offer_deps,
)

__all__ = [
    "BootstrapPolicy",
    "CloudWalletOfferDeps",
    "OfferBuildContext",
    "OfferCreateFailure",
    "OfferCreateOutcome",
    "OfferPostDeps",
    "OfferPostPersistRecord",
    "SignerOfferDeps",
    "LocalOfferBuildParams",
    "build_and_post_offer",
    "build_and_post_offer_cloud_wallet",
    "build_and_post_offer_signer",
    "default_cloud_wallet_offer_deps",
    "default_offer_post_deps",
    "default_signer_offer_deps",
    "execute_build_and_post_offer",
    "local_offer_params_from_context",
    "make_local_offer_create_fn",
    "persist_offer_post_records",
    "prepare_offer_build_context",
]
