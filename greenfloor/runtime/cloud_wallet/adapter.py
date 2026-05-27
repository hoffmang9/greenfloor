from __future__ import annotations

import json
from typing import Any, Protocol

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig

_JSON_OUTPUT_COMPACT = False


class SupportsWalletAssetsSeed(Protocol):
    """Minimal Cloud Wallet shape for ``seed_cloud_wallet_assets_cache``."""

    @property
    def vault_id(self) -> str: ...

    @property
    def _base_url(self) -> str: ...

    def _graphql(self, *, query: str, variables: dict[str, Any]) -> dict[str, Any]: ...


def _format_json_output(payload: object) -> str:
    if _JSON_OUTPUT_COMPACT:
        return json.dumps(payload, separators=(",", ":"))
    return json.dumps(payload, indent=2)


def _require_cloud_wallet_config(program: Any) -> CloudWalletConfig:
    if not program.cloud_wallet_base_url:
        raise ValueError("cloud_wallet.base_url is required")
    if not program.cloud_wallet_user_key_id:
        raise ValueError("cloud_wallet.user_key_id is required")
    if not program.cloud_wallet_private_key_pem_path:
        raise ValueError("cloud_wallet.private_key_pem_path is required")
    if not program.cloud_wallet_vault_id:
        raise ValueError("cloud_wallet.vault_id is required")
    return CloudWalletConfig(
        base_url=program.cloud_wallet_base_url,
        user_key_id=program.cloud_wallet_user_key_id,
        private_key_pem_path=program.cloud_wallet_private_key_pem_path,
        vault_id=program.cloud_wallet_vault_id,
        network=program.app_network,
        kms_key_id=program.cloud_wallet_kms_key_id or None,
        kms_region=program.cloud_wallet_kms_region or None,
        kms_public_key_hex=program.cloud_wallet_kms_public_key_hex or None,
    )


def new_cloud_wallet_adapter(program: Any) -> CloudWalletAdapter:
    return CloudWalletAdapter(_require_cloud_wallet_config(program))
