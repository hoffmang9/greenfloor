"""Single runtime composition root for offer execution orchestration."""

from __future__ import annotations

from greenfloor.runtime.local_offer import (
    build_daemon_action_offer_payload,
    make_local_offer_create_fn,
)
from greenfloor.runtime.offer_build_context import (
    OfferBuildContext,
    default_program_config_path,
    keyring_yaml_path_for_market,
    prepare_offer_build_context,
)
from greenfloor.runtime.offer_orchestration import (
    OfferCreateFailure,
    OfferCreateOutcome,
    OfferPostDeps,
    OfferPostPersistRecord,
    build_and_post_offer,
    default_offer_post_deps,
    execute_build_and_post_offer,
    persist_offer_post_records,
)
from greenfloor.runtime.offer_post_request import (
    ManagedOfferPostResult,
    OfferPostRequest,
    parse_managed_offer_post_result,
)
from greenfloor.runtime.offer_runtime import (
    SignerOfferDeps,
    build_and_post_offer_signer,
    default_signer_offer_deps,
)

__all__ = [
    "OfferBuildContext",
    "OfferCreateFailure",
    "OfferCreateOutcome",
    "OfferPostDeps",
    "OfferPostPersistRecord",
    "ManagedOfferPostResult",
    "OfferPostRequest",
    "SignerOfferDeps",
    "build_and_post_offer",
    "build_and_post_offer_signer",
    "build_daemon_action_offer_payload",
    "default_offer_post_deps",
    "default_program_config_path",
    "default_signer_offer_deps",
    "execute_build_and_post_offer",
    "keyring_yaml_path_for_market",
    "make_local_offer_create_fn",
    "parse_managed_offer_post_result",
    "persist_offer_post_records",
    "prepare_offer_build_context",
]
