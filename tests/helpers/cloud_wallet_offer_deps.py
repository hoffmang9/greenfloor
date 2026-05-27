from __future__ import annotations

from dataclasses import replace
from typing import Any

from greenfloor.runtime.cloud_wallet.deps import (
    CloudWalletOfferDeps,
    default_cloud_wallet_offer_deps,
)


def cloud_wallet_test_deps(**kwargs: Any) -> CloudWalletOfferDeps:
    """Build CloudWalletOfferDeps for tests with optional field overrides."""
    base = default_cloud_wallet_offer_deps()
    post_fields = {
        "initialize_manager_file_logging_fn",
        "resolve_maker_offer_fee_fn",
        "log_signed_offer_artifact_fn",
        "verify_offer_text_for_dexie_fn",
        "post_offer_phase_fn",
        "dexie_offer_view_url_fn",
        "dexie_adapter_cls",
        "splash_adapter_cls",
        "format_output_fn",
    }
    post_overrides = {key: kwargs.pop(key) for key in list(kwargs) if key in post_fields}
    post_deps = replace(base.post_deps, **post_overrides) if post_overrides else base.post_deps
    return replace(base, post_deps=post_deps, **kwargs)
