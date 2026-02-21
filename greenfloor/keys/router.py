from __future__ import annotations

from dataclasses import dataclass

from greenfloor.config.models import MarketConfig, SignerKeyConfig


@dataclass(slots=True)
class KeySelection:
    key_id: str
    market_id: str
    fingerprint: int | None = None
    keyring_yaml_path: str | None = None


def resolve_market_key(
    market: MarketConfig,
    allowed_key_ids: set[str] | None = None,
    signer_key_registry: dict[str, SignerKeyConfig] | None = None,
    required_network: str | None = None,
) -> KeySelection:
    key_id = market.signer_key_id.strip()
    if not key_id:
        raise ValueError(f"Market {market.market_id} is missing signer_key_id")
    if allowed_key_ids is not None and key_id not in allowed_key_ids:
        raise ValueError(
            f"Market {market.market_id} uses signer_key_id={key_id}, which is not allowed"
        )
    if signer_key_registry is not None:
        signer_key = signer_key_registry.get(key_id)
        if signer_key is None:
            raise ValueError(
                f"Market {market.market_id} uses signer_key_id={key_id}, which is not present in signer key registry"
            )
        if required_network and signer_key.network and signer_key.network != required_network:
            raise ValueError(
                f"Market {market.market_id} uses signer_key_id={key_id}, network mismatch ({signer_key.network} != {required_network})"
            )
        return KeySelection(
            key_id=key_id,
            market_id=market.market_id,
            fingerprint=signer_key.fingerprint,
            keyring_yaml_path=signer_key.keyring_yaml_path,
        )
    return KeySelection(key_id=key_id, market_id=market.market_id)
