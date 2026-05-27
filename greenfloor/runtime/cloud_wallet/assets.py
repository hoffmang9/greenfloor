from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

import greenfloor.asset_label_catalog as _asset_label_catalog
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.cloud_wallet_asset_cache import (
    load_wallet_assets_edges,
    save_wallet_assets_edges,
    wallet_assets_cache_path,
)
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.cloud_wallet.adapter import SupportsWalletAssetsSeed
from greenfloor.storage.sqlite import SqliteStore

_runtime_logger = logging.getLogger("greenfloor.manager")


def _wallet_asset_edges_for_resolve(
    *,
    wallet: CloudWalletAdapter,
    program_home_dir: str | None,
) -> list[dict[str, Any]]:
    """Return ``wallet.assets.edges`` from cache or Cloud Wallet GraphQL."""
    home_for_cache = str(program_home_dir or "").strip()
    base_url_for_cache = str(getattr(wallet, "_base_url", "") or "").strip()
    if home_for_cache and base_url_for_cache:
        cached = load_wallet_assets_edges(
            home_for_cache,
            base_url=base_url_for_cache,
            vault_id=str(wallet.vault_id),
        )
        if cached is not None:
            _runtime_logger.debug(
                "cloud_wallet_wallet_assets_cache_hit vault_id=%s",
                wallet.vault_id,
            )
            return cached
    query = """
query resolveWalletAssets($walletId: ID!) {
  wallet(id: $walletId) {
    assets {
      edges {
        node {
          assetId
          type
          displayName
          symbol
        }
      }
    }
  }
}
"""
    payload = wallet._graphql(query=query, variables={"walletId": wallet.vault_id})
    wallet_payload = payload.get("wallet") or {}
    assets_payload = wallet_payload.get("assets") or {}
    raw_edges = assets_payload.get("edges") or []
    edges = raw_edges if isinstance(raw_edges, list) else []
    if home_for_cache and base_url_for_cache and edges:
        save_wallet_assets_edges(
            home_for_cache,
            base_url=base_url_for_cache,
            vault_id=str(wallet.vault_id),
            edges=edges,
        )
    return edges


def seed_cloud_wallet_assets_cache(
    *,
    wallet: SupportsWalletAssetsSeed,
    program_home_dir: str,
) -> dict[str, Any]:
    """Fetch ``resolveWalletAssets`` once and write the disk catalog cache.

    Always hits the network (ignores any existing cache read). Operators use
    this to warm ``~/.greenfloor/cache/wallet_assets_*.json`` before starting
    the daemon or after vault asset changes.
    """
    home = str(program_home_dir).strip()
    base = str(getattr(wallet, "_base_url", "") or "").strip()
    if not home:
        raise ValueError("program_home_dir is required")
    if not base:
        raise ValueError("wallet API base_url is missing")
    vault_id = str(wallet.vault_id).strip()
    query = """
query resolveWalletAssets($walletId: ID!) {
  wallet(id: $walletId) {
    assets {
      edges {
        node {
          assetId
          type
          displayName
          symbol
        }
      }
    }
  }
}
"""
    payload = wallet._graphql(query=query, variables={"walletId": vault_id})
    wallet_payload = payload.get("wallet") or {}
    assets_payload = wallet_payload.get("assets") or {}
    raw_edges = assets_payload.get("edges") or []
    edges = raw_edges if isinstance(raw_edges, list) else []
    if not edges:
        raise RuntimeError("cloud_wallet_assets_seed_failed:empty_edges")
    save_wallet_assets_edges(
        home,
        base_url=base,
        vault_id=vault_id,
        edges=edges,
    )
    path = wallet_assets_cache_path(home, base_url=base, vault_id=vault_id)
    return {
        "cache_path": str(path),
        "edge_count": len(edges),
        "vault_id": vault_id,
        "base_url": base,
    }


def _resolve_asset_by_identifier(wallet: CloudWalletAdapter, hex_identifier: str) -> str | None:
    query = """
query resolveAssetByIdentifier($identifier: String) {
  asset(identifier: $identifier) {
    id
    type
  }
}
"""
    try:
        payload = wallet._graphql(query=query, variables={"identifier": hex_identifier})
    except Exception:
        return None
    asset = payload.get("asset")
    if not isinstance(asset, dict):
        return None
    global_id = str(asset.get("id", "")).strip()
    asset_type = str(asset.get("type", "")).strip().upper()
    if global_id.startswith("Asset_") and asset_type in {"CAT2", "CAT"}:
        return global_id
    return None


def resolve_cloud_wallet_asset_id(
    *,
    wallet: CloudWalletAdapter,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
    global_id_hint: str | None = None,
    allow_dexie_lookup: bool = True,
    program_home_dir: str | None = None,
) -> str:
    raw = canonical_asset_id.strip()
    if not raw:
        raise ValueError("asset_id must be non-empty")
    if _asset_label_catalog._canonical_is_cloud_global_id(raw):
        return raw
    if not hasattr(wallet, "_graphql"):
        return raw
    edges = _wallet_asset_edges_for_resolve(
        wallet=wallet,
        program_home_dir=program_home_dir,
    )
    crypto_asset_ids: list[str] = []
    cat_assets: list[dict[str, str]] = []
    for edge in edges:
        node = edge.get("node") if isinstance(edge, dict) else None
        if not isinstance(node, dict):
            continue
        asset_global_id = str(node.get("assetId", "")).strip()
        asset_type = str(node.get("type", "")).strip().upper()
        display_name = str(node.get("displayName", "")).strip()
        symbol = str(node.get("symbol", "")).strip()
        if not asset_global_id.startswith("Asset_"):
            continue
        if asset_type == "CRYPTOCURRENCY":
            crypto_asset_ids.append(asset_global_id)
        elif asset_type in {"CAT2", "CAT"}:
            cat_assets.append(
                {
                    "asset_id": asset_global_id,
                    "display_name": display_name,
                    "symbol": symbol,
                }
            )
    if canonical_is_xch(raw):
        hinted = str(global_id_hint or "").strip()
        if hinted and hinted in set(crypto_asset_ids):
            return hinted
        if len(crypto_asset_ids) == 1:
            return crypto_asset_ids[0]
        if len(crypto_asset_ids) == 0:
            raise RuntimeError("cloud_wallet_asset_resolution_failed:no_crypto_asset_found_for_xch")
        raise RuntimeError("cloud_wallet_asset_resolution_failed:ambiguous_crypto_asset_for_xch")
    if not cat_assets:
        raise RuntimeError(
            f"cloud_wallet_asset_resolution_failed:no_wallet_cat_asset_candidates_for:{raw}"
        )
    hinted = str(global_id_hint or "").strip()
    if hinted:
        cat_asset_ids = {str(row.get("asset_id", "")).strip() for row in cat_assets}
        if hinted in cat_asset_ids:
            return hinted
    canonical_hex = raw.lower()
    if _asset_label_catalog._is_hex_asset_id(canonical_hex):
        identifier_match = _resolve_asset_by_identifier(wallet, canonical_hex)
        if identifier_match is not None:
            return identifier_match
    preferred_labels: list[str] = []
    if symbol_hint:
        preferred_labels.append(symbol_hint)
    if not _asset_label_catalog._is_hex_asset_id(canonical_hex):
        direct_matches = _asset_label_catalog._wallet_label_matches_asset_ref(
            cat_assets=cat_assets, label=raw
        )
        if len(direct_matches) == 1:
            return direct_matches[0]
        if len(direct_matches) > 1:
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:ambiguous_wallet_cat_asset_for:{raw}"
            )
        if not allow_dexie_lookup:
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:unsupported_canonical_asset_id:{raw}"
            )
        token_row = _asset_label_catalog._dexie_lookup_token_for_symbol(
            asset_ref=raw, network=wallet.network
        )
        if token_row is None:
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:unsupported_canonical_asset_id:{raw}"
            )
        token_id = str(token_row.get("id", "")).strip().lower()
        if not _asset_label_catalog._is_hex_asset_id(token_id):
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:dexie_symbol_unresolved_to_cat_id:{raw}"
            )
        canonical_hex = token_id
    preferred_labels.extend(
        _asset_label_catalog._local_catalog_label_hints_for_asset_id(
            canonical_asset_id=canonical_hex
        )
    )
    dexie_token = (
        _asset_label_catalog._dexie_lookup_token_for_cat_id(
            canonical_cat_id_hex=canonical_hex,
            network=wallet.network,
        )
        if allow_dexie_lookup
        else None
    )
    if dexie_token is not None:
        preferred_labels.extend(
            [
                str(dexie_token.get("code", "")).strip(),
                str(dexie_token.get("name", "")).strip(),
                str(dexie_token.get("base_code", "")).strip(),
                str(dexie_token.get("base_name", "")).strip(),
                str(dexie_token.get("target_code", "")).strip(),
                str(dexie_token.get("target_name", "")).strip(),
            ]
        )
    preferred_labels = [label for label in preferred_labels if label]
    matched_assets: list[str] = []
    for label in preferred_labels:
        matched_assets.extend(
            _asset_label_catalog._wallet_label_matches_asset_ref(cat_assets=cat_assets, label=label)
        )
    unique_matches = sorted(set(matched_assets))
    if len(unique_matches) == 1:
        return unique_matches[0]
    if len(unique_matches) > 1:
        raise RuntimeError(
            f"cloud_wallet_asset_resolution_failed:ambiguous_wallet_cat_asset_for:{raw}"
        )
    if dexie_token is None and allow_dexie_lookup:
        raise RuntimeError(
            f"cloud_wallet_asset_resolution_failed:dexie_cat_metadata_not_found_for:{raw}"
        )
    if len(cat_assets) == 1:
        return cat_assets[0]["asset_id"]
    raise RuntimeError(f"cloud_wallet_asset_resolution_failed:unmatched_wallet_cat_asset_for:{raw}")


def resolve_cloud_wallet_offer_asset_ids(
    *,
    wallet: CloudWalletAdapter,
    base_asset_id: str,
    quote_asset_id: str,
    base_symbol_hint: str | None = None,
    quote_symbol_hint: str | None = None,
    base_global_id_hint: str | None = None,
    quote_global_id_hint: str | None = None,
    program_home_dir: str | None = None,
) -> tuple[str, str]:
    resolved_base = resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=base_asset_id,
        symbol_hint=(base_symbol_hint or "").strip() or str(base_asset_id).strip(),
        global_id_hint=(base_global_id_hint or "").strip() or None,
        program_home_dir=program_home_dir,
    )
    resolved_quote = resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=quote_asset_id,
        symbol_hint=(quote_symbol_hint or "").strip() or str(quote_asset_id).strip(),
        global_id_hint=(quote_global_id_hint or "").strip() or None,
        program_home_dir=program_home_dir,
    )
    if (
        resolved_base == resolved_quote
        and not canonical_is_xch(base_asset_id)
        and not canonical_is_xch(quote_asset_id)
        and not _asset_label_catalog._canonical_is_cloud_global_id(base_asset_id)
        and not _asset_label_catalog._canonical_is_cloud_global_id(quote_asset_id)
    ):
        raise RuntimeError(
            "cloud_wallet_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair"
        )
    return resolved_base, resolved_quote


def recent_market_resolved_asset_id_hints(
    *,
    program_home_dir: str,
    market_id: str,
) -> tuple[str | None, str | None]:
    db_path = (Path(program_home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()
    if not db_path.exists():
        return None, None
    store = SqliteStore(db_path)
    try:
        events = store.list_recent_audit_events(
            event_types=["strategy_offer_execution"],
            market_id=market_id,
            limit=200,
        )
    finally:
        store.close()
    for event in events:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        base_hint = str(payload.get("resolved_base_asset_id", "")).strip()
        quote_hint = str(payload.get("resolved_quote_asset_id", "")).strip()
        if base_hint.startswith("Asset_") and quote_hint.startswith("Asset_"):
            return base_hint, quote_hint
    return None, None
