"""Dexie token lookup, label matching, and local cats/markets YAML hints for asset resolution."""

from __future__ import annotations

from pathlib import Path

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.config.io import is_testnet, load_yaml
from greenfloor.hex_utils import is_hex_id, normalize_hex_id


def canonical_is_cloud_global_id(asset_id: str) -> bool:
    return asset_id.strip().startswith("Asset_")


def is_hex_asset_id(value: str) -> bool:
    return is_hex_id(value)


def normalize_label(value: str) -> str:
    return "".join(ch for ch in value.strip().lower() if ch.isalnum())


def label_tokens(value: str) -> list[str]:
    tokens: list[str] = []
    current: list[str] = []
    for ch in value.strip().lower():
        if ch.isalnum():
            current.append(ch)
        else:
            if current:
                tokens.append("".join(current))
                current = []
    if current:
        tokens.append("".join(current))
    return tokens


def labels_match(left: str, right: str) -> bool:
    a = normalize_label(left)
    b = normalize_label(right)
    if not a or not b:
        return False
    if a == b:
        return True
    if len(a) >= 5 and a in b:
        return True
    if len(b) >= 5 and b in a:
        return True
    left_tokens = {token for token in label_tokens(left) if len(token) >= 3}
    right_tokens = {token for token in label_tokens(right) if len(token) >= 3}
    return bool(left_tokens and right_tokens and len(left_tokens & right_tokens) >= 2)


def wallet_label_matches_asset_ref(
    *,
    cat_assets: list[dict[str, str]],
    label: str,
) -> list[str]:
    target = label.strip()
    if not target:
        return []
    matches: list[str] = []
    for cat in cat_assets:
        asset_id = cat.get("asset_id", "").strip()
        if not asset_id:
            continue
        display_name = cat.get("display_name", "").strip()
        symbol = cat.get("symbol", "").strip()
        if labels_match(display_name, target) or labels_match(symbol, target):
            matches.append(asset_id)
    return sorted(set(matches))


def resolve_dexie_base_url(network: str, explicit_base_url: str | None = None) -> str:
    if explicit_base_url and explicit_base_url.strip():
        return explicit_base_url.strip().rstrip("/")
    network_l = network.strip().lower()
    if network_l in {"mainnet", ""}:
        return "https://api.dexie.space"
    if is_testnet(network_l):
        return "https://api-testnet.dexie.space"
    raise ValueError(f"unsupported network for dexie posting: {network}")


def dexie_lookup_token_for_cat_id(*, canonical_cat_id_hex: str, network: str) -> dict | None:
    adapter = DexieAdapter(resolve_dexie_base_url(network, None))
    return adapter.lookup_token_by_cat_id(canonical_cat_id_hex)


def dexie_lookup_token_for_symbol(*, asset_ref: str, network: str) -> dict | None:
    adapter = DexieAdapter(resolve_dexie_base_url(network, None))
    return adapter.lookup_token_by_symbol(asset_ref, label_matcher=labels_match)


def normalize_hex_asset_id(asset_id: str) -> str:
    result = normalize_hex_id(asset_id)
    if result:
        return result
    normalized = str(asset_id).strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    return normalized


def local_catalog_label_hints_for_asset_id(*, canonical_asset_id: str) -> list[str]:
    canonical = canonical_asset_id.strip().lower()
    if not canonical:
        return []
    repo_root = Path(__file__).resolve().parents[1]
    cats_path = repo_root / "config" / "cats.yaml"
    markets_path = repo_root / "config" / "markets.yaml"
    try:
        cats_payload = load_yaml(cats_path) if cats_path.exists() else {}
        markets_payload = load_yaml(markets_path) if markets_path.exists() else {}
    except Exception:
        return []
    hints: list[str] = []
    cats_rows = cats_payload.get("cats") if isinstance(cats_payload, dict) else None
    if isinstance(cats_rows, list):
        for row in cats_rows:
            if not isinstance(row, dict):
                continue
            row_asset_id = str(row.get("asset_id", "")).strip().lower()
            if row_asset_id != canonical:
                continue
            for key in ("base_symbol", "name"):
                value = str(row.get(key, "")).strip()
                if value:
                    hints.append(value)
            aliases = row.get("aliases")
            if isinstance(aliases, list):
                for alias in aliases:
                    value = str(alias).strip()
                    if value:
                        hints.append(value)
    assets_rows = markets_payload.get("assets") if isinstance(markets_payload, dict) else None
    if isinstance(assets_rows, list):
        for row in assets_rows:
            if not isinstance(row, dict):
                continue
            row_asset_id = str(row.get("asset_id", "")).strip().lower()
            if row_asset_id != canonical:
                continue
            for key in ("base_symbol", "name"):
                value = str(row.get(key, "")).strip()
                if value:
                    hints.append(value)
    markets_rows = markets_payload.get("markets") if isinstance(markets_payload, dict) else None
    if isinstance(markets_rows, list):
        for row in markets_rows:
            if not isinstance(row, dict):
                continue
            base_asset = str(row.get("base_asset", "")).strip().lower()
            if base_asset != canonical:
                continue
            base_symbol = str(row.get("base_symbol", "")).strip()
            if base_symbol:
                hints.append(base_symbol)
    return sorted(set(hints))


# Backward-compatible underscored aliases for monkeypatch targets.
_canonical_is_cloud_global_id = canonical_is_cloud_global_id
_is_hex_asset_id = is_hex_asset_id
_normalize_label = normalize_label
_label_tokens = label_tokens
_labels_match = labels_match
_wallet_label_matches_asset_ref = wallet_label_matches_asset_ref
_resolve_dexie_base_url = resolve_dexie_base_url
_dexie_lookup_token_for_cat_id = dexie_lookup_token_for_cat_id
_dexie_lookup_token_for_symbol = dexie_lookup_token_for_symbol
_normalize_hex_asset_id = normalize_hex_asset_id
_local_catalog_label_hints_for_asset_id = local_catalog_label_hints_for_asset_id
