from __future__ import annotations

import collections.abc
import datetime as dt
import importlib
import json
import logging
import sys
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Protocol

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.asset_label_catalog import (
    _canonical_is_cloud_global_id,
    _dexie_lookup_token_for_cat_id,
    _dexie_lookup_token_for_symbol,
    _is_hex_asset_id,
    _local_catalog_label_hints_for_asset_id,
    _wallet_label_matches_asset_ref,
)
from greenfloor.cloud_wallet_asset_cache import (
    load_wallet_assets_edges,
    save_wallet_assets_edges,
    wallet_assets_cache_path,
)
from greenfloor.config.io import is_testnet
from greenfloor.core.offer_lifecycle import OfferLifecycleState
from greenfloor.hex_utils import (
    canonical_is_xch,
    default_mojo_multiplier_for_asset,
    normalize_hex_id,
)
from greenfloor.logging_setup import initialize_service_file_logging
from greenfloor.moderate_retry import (
    call_with_moderate_retry,
    poll_with_exponential_backoff_until,
)
from greenfloor.offer_bootstrap import plan_bootstrap_mixed_outputs
from greenfloor.offer_decode import (
    extract_coin_id_hints_from_offer_text as _extract_coin_id_hints_from_offer_text,
)
from greenfloor.runtime.coinset_runtime import (
    _resolve_taker_or_coin_operation_fee,
    resolve_maker_offer_fee,
)
from greenfloor.storage.sqlite import SqliteStore

_MANAGER_SERVICE_NAME = "manager"
_DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS = 4
_DEXIE_INVALID_OFFER_RETRY_INITIAL_DELAY_SECONDS = 1.0
_DEXIE_VISIBILITY_POST_MAX_ATTEMPTS = 3
_DEXIE_VISIBILITY_POST_DELAY_SECONDS = 2.0
_runtime_logger = logging.getLogger("greenfloor.manager")
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


def initialize_manager_file_logging(home_dir: str, *, log_level: str | None) -> None:
    initialize_service_file_logging(
        service_name=_MANAGER_SERVICE_NAME,
        home_dir=home_dir,
        log_level=log_level,
        service_logger=_runtime_logger,
    )


def _condition_has_offer_expiration(condition: object) -> bool:
    parse_names = (
        "parse_assert_before_seconds_relative",
        "parse_assert_before_seconds_absolute",
        "parse_assert_before_height_relative",
        "parse_assert_before_height_absolute",
    )
    for parse_name in parse_names:
        parse_fn = getattr(condition, parse_name, None)
        if not callable(parse_fn):
            continue
        try:
            if parse_fn() is not None:
                return True
        except Exception:
            continue
    return False


def _offer_has_expiration_condition(sdk: object, offer_text: str) -> bool:
    decode_offer = getattr(sdk, "decode_offer", None)
    if not callable(decode_offer):
        return False
    spend_bundle = decode_offer(offer_text)
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    for coin_spend in coin_spends:
        for condition in _extract_offer_conditions_from_coin_spend(sdk, coin_spend):
            if _condition_has_offer_expiration(condition):
                return True
    return False


def _extract_offer_conditions_from_coin_spend(sdk: object, coin_spend: object) -> list[object]:
    # Derive conditions from CLVM execution of puzzle reveal + solution.
    clvm_cls = getattr(sdk, "Clvm", None)
    if not callable(clvm_cls):
        return []
    puzzle_reveal = getattr(coin_spend, "puzzle_reveal", None)
    solution = getattr(coin_spend, "solution", None)
    if not isinstance(puzzle_reveal, bytes | bytearray | memoryview) or not isinstance(
        solution, bytes | bytearray | memoryview
    ):
        return []

    try:
        clvm = clvm_cls()
        deserialize_fn = getattr(clvm, "deserialize", None)
        if not callable(deserialize_fn):
            return []
        puzzle_program = deserialize_fn(bytes(puzzle_reveal))
        solution_program = deserialize_fn(bytes(solution))
        run_fn = getattr(puzzle_program, "run", None)
        if not callable(run_fn):
            return []
        run_output = run_fn(solution_program, 1_000_000_000_000, True)
        value = getattr(run_output, "value", None)
        if value is None:
            return []
        to_list_fn = getattr(value, "to_list", None)
        if callable(to_list_fn):
            parsed = to_list_fn() or []
            if isinstance(parsed, collections.abc.Iterable) and not isinstance(
                parsed, bytes | bytearray | str
            ):
                return list(parsed)
        if isinstance(value, collections.abc.Iterable) and not isinstance(
            value, bytes | bytearray | str
        ):
            return list(value)
    except Exception:
        return []
    return []


def _offer_has_duplicate_spent_coin_ids(sdk: object, offer_text: str) -> bool:
    decode_offer = getattr(sdk, "decode_offer", None)
    to_hex = getattr(sdk, "to_hex", None)
    if not callable(decode_offer) or not callable(to_hex):
        return False
    try:
        spend_bundle = decode_offer(offer_text)
    except Exception:
        return False
    coin_spends = getattr(spend_bundle, "coin_spends", None) or []
    seen: set[str] = set()
    for coin_spend in coin_spends:
        coin = getattr(coin_spend, "coin", None)
        if coin is None:
            continue
        coin_id_fn = getattr(coin, "coin_id", None)
        if not callable(coin_id_fn):
            continue
        try:
            coin_id_hex = str(to_hex(coin_id_fn())).strip().lower()
        except Exception:
            continue
        normalized = normalize_hex_id(coin_id_hex)
        if not normalized:
            continue
        if normalized in seen:
            return True
        seen.add(normalized)
    return False


def log_signed_offer_artifact(
    *,
    offer_text: str,
    ticker: str,
    amount: int,
    trading_pair: str,
    expiry: str,
) -> None:
    coin_id_hints = _extract_coin_id_hints_from_offer_text(offer_text)
    coin_id = coin_id_hints[0] if coin_id_hints else ""
    _runtime_logger.debug("signed_offer_file:%s", offer_text)
    _runtime_logger.info(
        "signed_offer_metadata:ticker=%s coinid=%s amount=%s trading_pair=%s expiry=%s",
        ticker,
        coin_id,
        amount,
        trading_pair,
        expiry,
    )


def verify_offer_text_for_dexie(offer_text: str) -> str | None:
    native_validated = False
    try:
        native = importlib.import_module("greenfloor_native")
    except Exception:
        native = None
    else:
        try:
            native.validate_offer(offer_text)
            native_validated = True
        except Exception as exc:
            return f"wallet_sdk_offer_validate_failed:{exc}"
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception as exc:
        if native_validated:
            return None
        return f"wallet_sdk_import_error:{exc}"
    try:
        decode_offer = getattr(sdk, "decode_offer", None)
        decode_available = callable(decode_offer)
        if not native_validated:
            validate_offer = getattr(sdk, "validate_offer", None)
            if callable(validate_offer):
                validate_offer(offer_text)
            else:
                verify_offer = getattr(sdk, "verify_offer", None)
                if not callable(verify_offer):
                    return "wallet_sdk_validate_offer_unavailable"
                if not bool(verify_offer(offer_text)):
                    return "wallet_sdk_offer_verify_false"
        if native_validated and not decode_available:
            return None
        if _offer_has_duplicate_spent_coin_ids(sdk, offer_text):
            return "wallet_sdk_offer_duplicate_spent_coin_ids"
        if not _offer_has_expiration_condition(sdk, offer_text):
            return "wallet_sdk_offer_missing_expiration"
    except Exception as exc:
        return f"wallet_sdk_offer_validate_failed:{exc}"
    return None


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
    if _canonical_is_cloud_global_id(raw):
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
    if _is_hex_asset_id(canonical_hex):
        identifier_match = _resolve_asset_by_identifier(wallet, canonical_hex)
        if identifier_match is not None:
            return identifier_match
    preferred_labels: list[str] = []
    if symbol_hint:
        preferred_labels.append(symbol_hint)
    if not _is_hex_asset_id(canonical_hex):
        direct_matches = _wallet_label_matches_asset_ref(cat_assets=cat_assets, label=raw)
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
        token_row = _dexie_lookup_token_for_symbol(asset_ref=raw, network=wallet.network)
        if token_row is None:
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:unsupported_canonical_asset_id:{raw}"
            )
        token_id = str(token_row.get("id", "")).strip().lower()
        if not _is_hex_asset_id(token_id):
            raise RuntimeError(
                f"cloud_wallet_asset_resolution_failed:dexie_symbol_unresolved_to_cat_id:{raw}"
            )
        canonical_hex = token_id
    preferred_labels.extend(
        _local_catalog_label_hints_for_asset_id(canonical_asset_id=canonical_hex)
    )
    dexie_token = (
        _dexie_lookup_token_for_cat_id(
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
        matched_assets.extend(_wallet_label_matches_asset_ref(cat_assets=cat_assets, label=label))
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
        and not _canonical_is_cloud_global_id(base_asset_id)
        and not _canonical_is_cloud_global_id(quote_asset_id)
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


def parse_iso8601(value: str) -> dt.datetime | None:
    raw = value.strip()
    if not raw:
        return None
    normalized = raw.replace("Z", "+00:00")
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=dt.UTC)
    return parsed.astimezone(dt.UTC)


def offer_markers(offers: list[dict]) -> set[str]:
    markers: set[str] = set()
    for offer in offers:
        offer_id = str(offer.get("offerId", "")).strip()
        if offer_id:
            markers.add(f"id:{offer_id}")
        bech32 = str(offer.get("bech32", "")).strip()
        if bech32:
            markers.add(f"bech32:{bech32}")
    return markers


def pick_new_offer_artifact(
    *,
    offers: list[dict],
    known_markers: set[str],
    min_created_at: dt.datetime | None = None,
    require_open_state: bool = False,
    prefer_newest: bool = True,
) -> str:
    candidates: list[tuple[dt.datetime, dt.datetime, str]] = []
    allowed_candidate_states = {"OPEN", "PENDING"}
    for offer in offers:
        state = str(offer.get("state", "")).strip().upper()
        if state not in allowed_candidate_states:
            continue
        if require_open_state and state != "OPEN":
            continue
        bech32 = str(offer.get("bech32", "")).strip()
        if not bech32.startswith("offer1"):
            continue
        offer_id = str(offer.get("offerId", "")).strip()
        markers = {f"bech32:{bech32}"}
        if offer_id:
            markers.add(f"id:{offer_id}")
        if markers.issubset(known_markers):
            continue
        created_at = parse_iso8601(str(offer.get("createdAt", "")).strip())
        if min_created_at is not None:
            if created_at is None or created_at < min_created_at:
                continue
        expires_at = parse_iso8601(str(offer.get("expiresAt", "")).strip())
        candidates.append(
            (
                created_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                expires_at or dt.datetime.min.replace(tzinfo=dt.UTC),
                bech32,
            )
        )
    if not candidates:
        return ""
    candidates.sort(key=lambda row: (row[0], row[1]), reverse=bool(prefer_newest))
    return candidates[0][2]


def wallet_get_wallet_offers(
    wallet: CloudWalletAdapter,
    *,
    is_creator: bool | None,
    states: list[str] | None,
) -> dict[str, Any]:
    return wallet.get_wallet(is_creator=is_creator, states=states, first=100)


def _safe_int(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def post_dexie_offer_with_invalid_offer_retry(
    *,
    dexie: DexieAdapter,
    offer_text: str,
    drop_only: bool,
    claim_rewards: bool,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> dict[str, Any]:
    if sleep_fn is None:
        sleep_fn = time.sleep
    attempt = 0
    sleep_seconds = _DEXIE_INVALID_OFFER_RETRY_INITIAL_DELAY_SECONDS
    while True:
        result = dexie.post_offer(
            offer_text,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
        )
        error = str(result.get("error", "")).strip()
        should_retry = (
            bool(error)
            and "dexie_http_error:400" in error
            and "Invalid Offer" in error
            and attempt < (_DEXIE_INVALID_OFFER_RETRY_MAX_ATTEMPTS - 1)
        )
        if not should_retry:
            return result
        attempt += 1
        sleep_fn(sleep_seconds)
        sleep_seconds = min(8.0, sleep_seconds * 2.0)


def verify_dexie_offer_visible_by_id(
    *,
    dexie: DexieAdapter,
    offer_id: str,
    max_attempts: int = 4,
    delay_seconds: float = 1.5,
    expected_offered_asset_id: str | None = None,
    expected_offered_symbol: str | None = None,
    expected_requested_asset_id: str | None = None,
    expected_requested_symbol: str | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> str | None:
    if sleep_fn is None:
        sleep_fn = time.sleep
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return "dexie_offer_missing_id_after_publish"
    attempts = max(1, int(max_attempts))
    last_error = "dexie_offer_not_visible_after_publish"
    for attempt in range(1, attempts + 1):
        try:
            payload = dexie.get_offer(clean_offer_id)
        except Exception as exc:
            last_error = f"dexie_get_offer_error:{exc}"
            if attempt < attempts:
                sleep_fn(delay_seconds)
            continue
        offer_payload = payload.get("offer") if isinstance(payload, dict) else None
        visible_id = (
            str(offer_payload.get("id", "")).strip() if isinstance(offer_payload, dict) else ""
        )
        if visible_id == clean_offer_id:
            if isinstance(offer_payload, dict):
                offered = offer_payload.get("offered")
                requested = offer_payload.get("requested")
                if expected_offered_asset_id and isinstance(offered, list):
                    expected_asset = str(expected_offered_asset_id).strip().lower()
                    expected_symbol = str(expected_offered_symbol or "").strip().lower()
                    found = False
                    for row in offered:
                        if not isinstance(row, dict):
                            continue
                        asset_id = str(row.get("id", "")).strip().lower()
                        code = str(row.get("code", "")).strip().lower()
                        name = str(row.get("name", "")).strip().lower()
                        if asset_id == expected_asset or (
                            expected_symbol and (code == expected_symbol or name == expected_symbol)
                        ):
                            found = True
                            break
                    if not found:
                        return (
                            "dexie_offer_offered_asset_missing:"
                            f"expected_asset={expected_offered_asset_id}:"
                            f"expected_symbol={expected_offered_symbol}"
                        )
                if expected_requested_asset_id and isinstance(requested, list):
                    expected_asset = str(expected_requested_asset_id).strip().lower()
                    expected_symbol = str(expected_requested_symbol or "").strip().lower()
                    found = False
                    for row in requested:
                        if not isinstance(row, dict):
                            continue
                        asset_id = str(row.get("id", "")).strip().lower()
                        code = str(row.get("code", "")).strip().lower()
                        name = str(row.get("name", "")).strip().lower()
                        if asset_id == expected_asset or (
                            expected_symbol and (code == expected_symbol or name == expected_symbol)
                        ):
                            found = True
                            break
                    if not found:
                        return (
                            "dexie_offer_requested_asset_missing:"
                            f"expected_asset={expected_requested_asset_id}:"
                            f"expected_symbol={expected_requested_symbol}"
                        )
            return None
        last_error = "dexie_offer_visibility_payload_mismatch"
        if attempt < attempts:
            sleep_fn(delay_seconds)
    return last_error


def is_transient_dexie_visibility_404_error(error: str) -> bool:
    normalized = str(error).strip().lower()
    return (
        "dexie_get_offer_error" in normalized and "404" in normalized
    ) or "dexie_http_error:404" in normalized


def _is_transient_cloud_wallet_list_coins_error(error: str) -> bool:
    normalized = str(error).strip().lower()
    if not normalized:
        return False
    transient_markers = (
        "cloud_wallet_http_error:504",
        "cloud_wallet_http_error:503",
        "cloud_wallet_network_error",
        "http error 504",
        "http error 503",
        "gateway timeout",
        "service unavailable",
        "timed out",
        "timeout",
        "temporary failure",
        "connection reset",
        "connection refused",
        "remote end closed connection",
    )
    return any(marker in normalized for marker in transient_markers)


def _coinset_coin_url(*, coin_name: str, network: str = "mainnet") -> str:
    base = "https://testnet11.coinset.org" if is_testnet(network) else "https://coinset.org"
    return f"{base}/coin/{coin_name.strip()}"


def _coinset_reconcile_coin_state(*, network: str, coin_name: str) -> dict[str, str]:
    adapter = CoinsetAdapter(None, network=network)
    try:
        record = call_with_moderate_retry(
            action="coinset_get_coin_record_by_name",
            call=lambda: adapter.get_coin_record_by_name(coin_name_hex=coin_name),
        )
    except Exception as exc:
        return {"reconcile": "error", "error": str(exc)}
    if not isinstance(record, dict):
        return {"reconcile": "not_found"}
    confirmed_height = _safe_int(record.get("confirmed_block_index"))
    spent_height = _safe_int(record.get("spent_block_index"))
    return {
        "reconcile": "ok",
        "confirmed_block_index": str(confirmed_height if confirmed_height is not None else -1),
        "spent_block_index": str(spent_height if spent_height is not None else -1),
        "coinbase": str(bool(record.get("coinbase", False))).lower(),
    }


def _coinset_peak_height(*, network: str) -> int | None:
    adapter = CoinsetAdapter(None, network=network)
    state = call_with_moderate_retry(
        action="coinset_get_blockchain_state",
        call=adapter.get_blockchain_state,
    )
    if not isinstance(state, dict):
        return None
    candidates = [state.get("peak_height"), state.get("peakHeight")]
    peak = state.get("peak")
    if isinstance(peak, dict):
        candidates.extend([peak.get("height"), peak.get("peak_height")])
    for candidate in candidates:
        parsed = _safe_int(candidate)
        if parsed is not None and parsed >= 0:
            return parsed
    return None


def _watch_reorg_risk_with_coinset(
    *,
    network: str,
    confirmed_block_index: int,
    additional_blocks: int,
    warning_interval_seconds: int,
    timeout_seconds: int = 60 * 60,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    target_height = int(confirmed_block_index) + int(additional_blocks)
    events.append(
        {
            "event": "reorg_watch_started",
            "confirmed_block_index": str(confirmed_block_index),
            "target_height": str(target_height),
        }
    )
    start = time.monotonic()
    next_warning = warning_interval_seconds
    sleep_seconds = 8.0
    while True:
        elapsed = int(time.monotonic() - start)
        peak_height = _coinset_peak_height(network=network)
        if peak_height is None:
            events.append(
                {
                    "event": "reorg_watch_skipped",
                    "reason": "coinset_peak_height_unavailable",
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        remaining = target_height - peak_height
        if remaining <= 0:
            events.append(
                {
                    "event": "reorg_watch_complete",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        if elapsed >= timeout_seconds:
            events.append(
                {
                    "event": "reorg_watch_timeout",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "remaining_blocks": str(remaining),
                    "elapsed_seconds": str(elapsed),
                }
            )
            return events
        if elapsed >= next_warning:
            events.append(
                {
                    "event": "reorg_watch_warning",
                    "peak_height": str(peak_height),
                    "target_height": str(target_height),
                    "remaining_blocks": str(remaining),
                    "elapsed_seconds": str(elapsed),
                }
            )
            next_warning += warning_interval_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def poll_offer_artifact_until_available(
    *,
    wallet: CloudWalletAdapter,
    known_markers: set[str],
    timeout_seconds: int,
    min_created_at: dt.datetime | None = None,
    require_open_state: bool = False,
    states: tuple[str, ...] | None = ("OPEN", "PENDING"),
    prefer_newest: bool = True,
    wallet_get_wallet_offers_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> str:
    if wallet_get_wallet_offers_fn is None:
        wallet_get_wallet_offers_fn = wallet_get_wallet_offers
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic

    def _on_tick(elapsed: int) -> str | None:
        wallet_payload = retry_fn(
            action="wallet_get_wallet",
            call=lambda: wallet_get_wallet_offers_fn(
                wallet,
                is_creator=True,
                states=list(states) if states is not None else None,
            ),
            elapsed_seconds=elapsed,
        )
        offers = wallet_payload.get("offers", [])
        if isinstance(offers, list):
            offer_text = pick_new_offer_artifact(
                offers=offers,
                known_markers=known_markers,
                min_created_at=min_created_at,
                require_open_state=require_open_state,
                prefer_newest=prefer_newest,
            )
            if offer_text:
                return offer_text
        return None

    return poll_with_exponential_backoff_until(
        monotonic_fn=monotonic_fn,
        sleep_fn=sleep_fn,
        timeout_seconds=timeout_seconds,
        initial_sleep=2.0,
        max_sleep=20.0,
        sleep_multiplier=1.5,
        on_tick=_on_tick,
        timeout_error="cloud_wallet_offer_artifact_timeout",
    )


def poll_offer_artifact_by_signature_request(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    known_markers: set[str],
    timeout_seconds: int,
    min_created_at: dt.datetime | None = None,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> str:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic

    def _on_tick(elapsed: int) -> str | None:
        payload = retry_fn(
            action="wallet_get_signature_request_offer",
            call=lambda: wallet.get_signature_request_offer(
                signature_request_id=signature_request_id
            ),
            elapsed_seconds=elapsed,
        )
        bech32 = str(payload.get("bech32", "")).strip()
        offer_id = str(payload.get("offer_id", "")).strip()
        offer_state = str(payload.get("state", "")).strip().upper()
        created_at = parse_iso8601(str(payload.get("created_at", "")).strip())
        markers = {f"bech32:{bech32}"} if bech32 else set()
        if offer_id:
            markers.add(f"id:{offer_id}")
        markers_already_known = bool(markers) and markers.issubset(known_markers)
        created_at_gte_min = (
            bool(created_at and min_created_at and created_at >= min_created_at)
            if min_created_at is not None
            else True
        )
        if (
            bech32.startswith("offer1")
            and offer_state in {"OPEN", "PENDING", "SETTLED"}
            and not markers_already_known
            and created_at_gte_min
        ):
            return bech32
        return None

    return poll_with_exponential_backoff_until(
        monotonic_fn=monotonic_fn,
        sleep_fn=sleep_fn,
        timeout_seconds=timeout_seconds,
        initial_sleep=2.0,
        max_sleep=20.0,
        sleep_multiplier=1.5,
        on_tick=_on_tick,
        timeout_error="cloud_wallet_offer_artifact_timeout",
    )


def poll_signature_request_until_not_unsigned(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    timeout_seconds: int,
    warning_interval_seconds: int,
    retry_fn: collections.abc.Callable[..., Any] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
    monotonic_fn: collections.abc.Callable[[], float] | None = None,
) -> tuple[str, list[dict[str, str]]]:
    if retry_fn is None:
        retry_fn = call_with_moderate_retry
    if sleep_fn is None:
        sleep_fn = time.sleep
    if monotonic_fn is None:
        monotonic_fn = time.monotonic
    events: list[dict[str, str]] = []
    start = monotonic_fn()
    next_warning = warning_interval_seconds
    warning_count = 0
    next_heartbeat = 5
    sleep_seconds = 2.0
    while True:
        elapsed = int(monotonic_fn() - start)
        status_payload = retry_fn(
            action="wallet_get_signature_request",
            call=lambda: wallet.get_signature_request(signature_request_id=signature_request_id),
            elapsed_seconds=elapsed,
            events=events,
        )
        status = str(status_payload.get("status", "")).strip().upper()
        if status and status != "UNSIGNED":
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            print(
                f"signature submitted: {signature_request_id} status={status}",
                file=sys.stderr,
                flush=True,
            )
            return status, events
        if elapsed >= next_heartbeat:
            print(".", end="", file=sys.stderr, flush=True)
            next_heartbeat += 5
        if elapsed >= timeout_seconds:
            raise RuntimeError("signature_request_timeout_waiting_for_signature")
        if elapsed >= next_warning:
            warning_count += 1
            events.append(
                {
                    "event": "signature_wait_warning",
                    "elapsed_seconds": str(elapsed),
                    "signing_state_age_seconds": str(elapsed),
                    "message": "still_waiting_on_user_signature",
                    "wait_reason": "waiting_on_user_signature",
                    "warning_count": str(warning_count),
                }
            )
            if warning_count >= 2:
                events.append(
                    {
                        "event": "signature_wait_escalation",
                        "elapsed_seconds": str(elapsed),
                        "message": "extended_user_signature_delay",
                        "wait_reason": "waiting_on_user_signature",
                        "warning_count": str(warning_count),
                    }
                )
            next_warning += warning_interval_seconds
        sleep_fn(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _coin_asset_id(coin: dict) -> str:
    asset_raw = coin.get("asset")
    if isinstance(asset_raw, dict):
        return str(asset_raw.get("id", "xch")).strip() or "xch"
    if isinstance(asset_raw, str):
        return asset_raw.strip() or "xch"
    return "xch"


def wait_for_mempool_then_confirmation(
    *,
    wallet: CloudWalletAdapter,
    network: str,
    initial_coin_ids: set[str],
    asset_id: str | None = None,
    mempool_warning_seconds: int,
    confirmation_warning_seconds: int,
    timeout_seconds: int | None = None,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    seen_pending = False
    next_heartbeat = 5
    sleep_seconds = 2.0
    next_mempool_warning = mempool_warning_seconds
    next_confirmation_warning = confirmation_warning_seconds
    target_asset = (
        asset_id.strip().lower() if isinstance(asset_id, str) and asset_id.strip() else None
    )
    while True:
        elapsed = int(time.monotonic() - start)
        coins = call_with_moderate_retry(
            action="wallet_list_coins",
            call=lambda: wallet.list_coins(include_pending=True),
            elapsed_seconds=elapsed,
            events=events,
        )
        pending = [
            c
            for c in coins
            if target_asset is None or _coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() in {"PENDING", "MEMPOOL"}
        ]
        confirmed = [
            c
            for c in coins
            if target_asset is None or _coin_asset_id(c).lower() == target_asset
            if str(c.get("id", "")).strip() not in initial_coin_ids
            if str(c.get("state", "")).strip().upper() not in {"PENDING", "MEMPOOL"}
        ]
        if pending and not seen_pending:
            seen_pending = True
            sample = str(pending[0].get("name", pending[0].get("id", ""))).strip()
            sample_id = str(pending[0].get("id", "")).strip()
            coinset_url = _coinset_coin_url(coin_name=sample, network=network)
            reconcile = _coinset_reconcile_coin_state(network=network, coin_name=sample)
            events.append(
                {
                    "event": "in_mempool",
                    "coin_id": sample_id,
                    "coin_name": sample,
                    "coinset_url": coinset_url,
                    "elapsed_seconds": str(elapsed),
                    "wait_reason": "waiting_for_mempool_admission",
                    **reconcile,
                }
            )
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            print(f"in mempool: {coinset_url}", file=sys.stderr, flush=True)
        if confirmed:
            sample_confirmed = str(confirmed[0].get("name", confirmed[0].get("id", ""))).strip()
            confirmation_reconcile = _coinset_reconcile_coin_state(
                network=network, coin_name=sample_confirmed
            )
            confirmed_height = _safe_int(confirmation_reconcile.get("confirmed_block_index"))
            events.append(
                {
                    "event": "confirmed",
                    "coin_name": sample_confirmed,
                    "coinset_url": _coinset_coin_url(coin_name=sample_confirmed, network=network),
                    "elapsed_seconds": str(elapsed),
                    "wait_reason": "waiting_for_confirmation",
                    **confirmation_reconcile,
                }
            )
            if confirmed_height is not None and confirmed_height >= 0:
                events.extend(
                    _watch_reorg_risk_with_coinset(
                        network=network,
                        confirmed_block_index=confirmed_height,
                        additional_blocks=6,
                        warning_interval_seconds=15 * 60,
                    )
                )
            if next_heartbeat > 5:
                print("", file=sys.stderr, flush=True)
            return events
        if elapsed >= next_heartbeat:
            print(".", end="", file=sys.stderr, flush=True)
            next_heartbeat += 5
        if timeout_seconds is not None and timeout_seconds > 0 and elapsed >= timeout_seconds:
            raise RuntimeError("confirmation_wait_timeout")
        if not seen_pending and elapsed >= next_mempool_warning:
            events.append({"event": "mempool_wait_warning", "elapsed_seconds": str(elapsed)})
            next_mempool_warning += mempool_warning_seconds
        if seen_pending and elapsed >= next_confirmation_warning:
            events.append({"event": "confirmation_wait_warning", "elapsed_seconds": str(elapsed)})
            next_confirmation_warning += confirmation_warning_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _is_spendable_coin(coin: dict) -> bool:
    if bool(coin.get("isLocked", False)):
        return False
    coin_state = str(coin.get("state", "")).strip().upper()
    if not coin_state:
        return False
    if coin_state in {
        "PENDING",
        "MEMPOOL",
        "SPENT",
        "SPENDING",
        "LOCKED",
        "RESERVED",
        "UNCONFIRMED",
    }:
        return False
    return coin_state in {"CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"}


def normalize_offer_side(value: str | None) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def dexie_offer_view_url(*, dexie_base_url: str, offer_id: str) -> str:
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return ""
    parsed = urllib.parse.urlparse(str(dexie_base_url).strip())
    host = parsed.netloc.strip().lower()
    if not host:
        return ""
    if host.startswith("api-testnet."):
        host = host[len("api-") :]
    elif host.startswith("api."):
        host = host[len("api.") :]
    return f"https://{host}/offers/{urllib.parse.quote(clean_offer_id)}"


def resolve_offer_expiry_for_market(market: Any) -> tuple[str, int]:
    pricing = dict(getattr(market, "pricing", {}) or {})
    value_raw = pricing.get("strategy_offer_expiry_minutes")
    try:
        value = int(value_raw or 0)
    except (TypeError, ValueError):
        value = 0
    if value > 0:
        return "minutes", value
    return "minutes", 10


def _bootstrap_fee_cost_for_output_count(output_count: int) -> int:
    count = max(1, int(output_count))
    return 1_000_000 + max(0, count - 1) * 250_000


def _resolve_bootstrap_split_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    output_count: int,
) -> tuple[int, str, str | None]:
    fee_cost = _bootstrap_fee_cost_for_output_count(output_count)
    spend_count = max(1, int(output_count))
    try:
        fee_mojos, fee_source = _resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
            fee_cost=fee_cost,
            spend_count=spend_count,
        )
        return int(fee_mojos), fee_source, None
    except Exception as exc:
        fallback_fee = max(0, int(minimum_fee_mojos))
        return fallback_fee, "config_minimum_fee_fallback", str(exc)


@dataclass(frozen=True, slots=True)
class _BootstrapLadderEntry:
    size_base_units: int
    target_count: int
    split_buffer_count: int


def ensure_offer_bootstrap_denominations(
    *,
    program: Any,
    market: Any,
    wallet: CloudWalletAdapter,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    quote_price: float,
    action_side: str = "sell",
    bootstrap_signature_wait_timeout_seconds: int = 45,
    bootstrap_signature_warning_interval_seconds: int = 30,
    bootstrap_wait_timeout_seconds: int = 120,
    bootstrap_wait_mempool_warning_seconds: int = 30,
    bootstrap_wait_confirmation_warning_seconds: int = 60,
    plan_bootstrap_mixed_outputs_fn: collections.abc.Callable[..., Any] | None = None,
    resolve_bootstrap_split_fee_fn: collections.abc.Callable[..., tuple[int, str, str | None]]
    | None = None,
    split_coins_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    poll_signature_request_until_not_unsigned_fn: collections.abc.Callable[
        ..., tuple[str, list[dict[str, str]]]
    ]
    | None = None,
    wait_for_mempool_then_confirmation_fn: collections.abc.Callable[..., list[dict[str, str]]]
    | None = None,
    is_spendable_coin_fn: collections.abc.Callable[[dict], bool] | None = None,
) -> dict[str, Any]:
    if plan_bootstrap_mixed_outputs_fn is None:
        plan_bootstrap_mixed_outputs_fn = plan_bootstrap_mixed_outputs
    if resolve_bootstrap_split_fee_fn is None:
        resolve_bootstrap_split_fee_fn = _resolve_bootstrap_split_fee
    if split_coins_fn is None:
        split_coins_fn = getattr(wallet, "split_coins", None)
    if poll_signature_request_until_not_unsigned_fn is None:
        poll_signature_request_until_not_unsigned_fn = poll_signature_request_until_not_unsigned
    if wait_for_mempool_then_confirmation_fn is None:
        wait_for_mempool_then_confirmation_fn = wait_for_mempool_then_confirmation
    if is_spendable_coin_fn is None:
        is_spendable_coin_fn = _is_spendable_coin
    side = normalize_offer_side(action_side)
    ladders = getattr(market, "ladders", {}) or {}
    side_ladder = list(ladders.get(side, []) or []) if isinstance(ladders, dict) else []
    if not side_ladder:
        return {"status": "skipped", "reason": f"missing_{side}_ladder"}
    pricing = dict(getattr(market, "pricing", {}) or {})
    quote_unit_multiplier = int(
        pricing.get(
            "quote_unit_mojo_multiplier",
            default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
        )
    )
    if side == "buy":
        ladder_for_split = []
        for entry in side_ladder:
            quote_amount = int(
                round(float(entry.size_base_units) * float(quote_price) * quote_unit_multiplier)
            )
            if quote_amount <= 0:
                continue
            ladder_for_split.append(
                _BootstrapLadderEntry(
                    size_base_units=quote_amount,
                    target_count=int(entry.target_count),
                    split_buffer_count=int(entry.split_buffer_count),
                )
            )
        split_asset_id = str(resolved_quote_asset_id).strip()
    else:
        ladder_for_split = side_ladder
        split_asset_id = str(resolved_base_asset_id).strip()
    if not split_asset_id:
        return {"status": "skipped", "reason": f"missing_{side}_asset_for_bootstrap"}
    if not hasattr(wallet, "list_coins"):
        return {
            "status": "skipped",
            "reason": "wallet_list_coins_unavailable_for_bootstrap",
            "fallback_to_cloud_wallet_offer_split": True,
        }
    asset_scoped_coins = wallet.list_coins(asset_id=split_asset_id, include_pending=True)
    spendable_asset_coins = [coin for coin in asset_scoped_coins if is_spendable_coin_fn(coin)]
    bootstrap_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=spendable_asset_coins,
    )
    if bootstrap_plan is None:
        return {"status": "skipped", "reason": "already_ready"}
    fee_mojos, fee_source, fee_lookup_error = resolve_bootstrap_split_fee_fn(
        network=str(program.app_network),
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        output_count=len(bootstrap_plan.output_amounts_base_units),
    )
    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in asset_scoped_coins if str(c.get("id", "")).strip()
    }
    selected_deficit = max(
        bootstrap_plan.deficits,
        key=lambda row: (int(row.size_base_units), int(row.deficit_count)),
    )
    amount_per_coin = int(selected_deficit.size_base_units)
    desired_coin_count = max(2, int(selected_deficit.deficit_count))
    max_coin_count = int(bootstrap_plan.source_amount) // max(1, amount_per_coin)
    number_of_coins = min(desired_coin_count, max_coin_count)
    if number_of_coins < 2:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:insufficient_source_coin_for_cloud_wallet_split",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "target_size_base_units": amount_per_coin,
                "requested_coin_count": desired_coin_count,
                "max_coin_count_from_source": max_coin_count,
            },
        }
    if split_coins_fn is None:
        return {"status": "failed", "reason": "split_coins_not_available"}
    try:
        split_result = split_coins_fn(
            coin_ids=[bootstrap_plan.source_coin_id],
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=int(fee_mojos),
        )
    except Exception as exc:
        return {
            "status": "failed",
            "reason": f"bootstrap_failed:cloud_wallet_split_error:{exc}",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "target_size_base_units": amount_per_coin,
                "coin_count": number_of_coins,
            },
        }
    signature_request_id = str(split_result.get("signature_request_id", "")).strip()
    if not signature_request_id:
        return {
            "status": "failed",
            "reason": "bootstrap_failed:missing_signature_request_id",
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }
    signature_events: list[dict[str, str]] = []
    try:
        signature_state, signature_events = poll_signature_request_until_not_unsigned_fn(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=max(5, int(bootstrap_signature_wait_timeout_seconds)),
            warning_interval_seconds=max(5, int(bootstrap_signature_warning_interval_seconds)),
        )
    except Exception as exc:
        return {
            "status": "failed",
            "reason": "bootstrap_signature_wait_failed",
            "signature_request_id": signature_request_id,
            "signature_wait_error": str(exc),
            "signature_wait_events": signature_events,
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
        }
    wait_events: list[dict[str, str]] = []
    wait_error: str | None = None
    try:
        wait_events = wait_for_mempool_then_confirmation_fn(
            wallet=wallet,
            network=str(program.app_network),
            initial_coin_ids=existing_coin_ids,
            asset_id=split_asset_id,
            mempool_warning_seconds=max(10, int(bootstrap_wait_mempool_warning_seconds)),
            confirmation_warning_seconds=max(10, int(bootstrap_wait_confirmation_warning_seconds)),
            timeout_seconds=max(10, int(bootstrap_wait_timeout_seconds)),
        )
    except Exception as exc:
        wait_error = str(exc)
        return {
            "status": "failed",
            "reason": "bootstrap_wait_failed",
            "wait_error": wait_error,
            "fallback_to_cloud_wallet_offer_split": True,
            "fee_mojos": int(fee_mojos),
            "fee_source": fee_source,
            "fee_lookup_error": fee_lookup_error,
            "plan": {
                "source_coin_id": bootstrap_plan.source_coin_id,
                "source_amount": bootstrap_plan.source_amount,
                "output_count": len(bootstrap_plan.output_amounts_base_units),
                "total_output_amount": bootstrap_plan.total_output_amount,
                "change_amount": bootstrap_plan.change_amount,
            },
            "signature_request_id": signature_request_id,
            "signature_state": signature_state,
            "signature_wait_events": signature_events,
            "wait_events": wait_events,
        }
    refreshed_asset_coins = wallet.list_coins(asset_id=split_asset_id, include_pending=True)
    refreshed_spendable = [coin for coin in refreshed_asset_coins if is_spendable_coin_fn(coin)]
    remaining_plan = plan_bootstrap_mixed_outputs_fn(
        sell_ladder=ladder_for_split,
        spendable_coins=refreshed_spendable,
    )
    return {
        "status": "executed",
        "reason": "bootstrap_submitted",
        "ready": remaining_plan is None,
        "fee_mojos": int(fee_mojos),
        "fee_source": fee_source,
        "fee_lookup_error": fee_lookup_error,
        "wait_error": wait_error,
        "plan": {
            "source_coin_id": bootstrap_plan.source_coin_id,
            "source_amount": bootstrap_plan.source_amount,
            "output_count": len(bootstrap_plan.output_amounts_base_units),
            "total_output_amount": bootstrap_plan.total_output_amount,
            "change_amount": bootstrap_plan.change_amount,
            "deficits": [
                {
                    "size_base_units": d.size_base_units,
                    "required_count": d.required_count,
                    "current_count": d.current_count,
                    "deficit_count": d.deficit_count,
                }
                for d in bootstrap_plan.deficits
            ],
        },
        "signature_request_id": signature_request_id,
        "signature_state": signature_state,
        "signature_wait_events": signature_events,
        "wait_events": wait_events,
    }


def cloud_wallet_create_offer_phase(
    *,
    wallet: CloudWalletAdapter,
    market: Any,
    size_base_units: int,
    quote_price: float,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    offer_fee_mojos: int,
    split_input_coins_fee: int,
    expiry_unit: str,
    expiry_value: int,
    action_side: str = "sell",
    signature_wait_timeout_seconds: int = 120,
    signature_wait_warning_interval_seconds: int = 60,
    wallet_get_wallet_offers_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    poll_signature_request_until_not_unsigned_fn: collections.abc.Callable[..., Any] | None = None,
) -> dict[str, Any]:
    if wallet_get_wallet_offers_fn is None:
        wallet_get_wallet_offers_fn = wallet_get_wallet_offers
    if poll_signature_request_until_not_unsigned_fn is None:
        poll_signature_request_until_not_unsigned_fn = poll_signature_request_until_not_unsigned
    side = normalize_offer_side(action_side)
    prior_wallet_payload = wallet_get_wallet_offers_fn(
        wallet,
        is_creator=True,
        states=["OPEN", "PENDING"],
    )
    prior_offers = prior_wallet_payload.get("offers", [])
    known_offer_markers = offer_markers(prior_offers if isinstance(prior_offers, list) else [])
    offer_request_started_at = dt.datetime.now(dt.UTC)
    offer_amount = int(
        size_base_units
        * int(
            (market.pricing or {}).get(
                "base_unit_mojo_multiplier",
                default_mojo_multiplier_for_asset(str(resolved_base_asset_id)),
            )
        )
    )
    request_amount = int(
        round(
            float(size_base_units)
            * float(quote_price)
            * int(
                (market.pricing or {}).get(
                    "quote_unit_mojo_multiplier",
                    default_mojo_multiplier_for_asset(str(resolved_quote_asset_id)),
                )
            )
        )
    )
    if request_amount <= 0:
        raise ValueError("request_amount must be positive")
    if side == "buy":
        offered = [{"assetId": resolved_quote_asset_id, "amount": request_amount}]
        requested = [{"assetId": resolved_base_asset_id, "amount": offer_amount}]
        spend_asset_id = str(resolved_quote_asset_id).strip()
        required_spendable_amount = int(request_amount)
    else:
        offered = [{"assetId": resolved_base_asset_id, "amount": offer_amount}]
        requested = [{"assetId": resolved_quote_asset_id, "amount": request_amount}]
        spend_asset_id = str(resolved_base_asset_id).strip()
        required_spendable_amount = int(offer_amount)
    if hasattr(wallet, "list_coins") and spend_asset_id:
        try:
            asset_scoped_coins = wallet.list_coins(asset_id=spend_asset_id, include_pending=True)
        except Exception as exc:
            if _is_transient_cloud_wallet_list_coins_error(str(exc)):
                _runtime_logger.warning(
                    "cloud_wallet_create_offer_precheck_skipped_due_to_transient_list_coins_error "
                    "side=%s asset_id=%s required_amount=%s error=%s",
                    side,
                    spend_asset_id,
                    int(required_spendable_amount),
                    str(exc),
                )
                asset_scoped_coins = []
            else:
                raise
        if asset_scoped_coins:
            spendable_amount = sum(
                int(coin.get("amount", 0))
                for coin in asset_scoped_coins
                if isinstance(coin, dict) and _is_spendable_coin(coin)
            )
            if spendable_amount < required_spendable_amount:
                raise RuntimeError(
                    "cloud_wallet_offer_insufficient_spendable_balance:"
                    f"side={side}:required={required_spendable_amount}:"
                    f"available={spendable_amount}:asset_id={spend_asset_id}"
                )
    expires_at = (
        dt.datetime.now(dt.UTC) + dt.timedelta(**{expiry_unit: int(expiry_value)})
    ).isoformat()
    create_result = wallet.create_offer(
        offered=offered,
        requested=requested,
        fee=offer_fee_mojos,
        expires_at_iso=expires_at,
        split_input_coins=False,
        split_input_coins_fee=0,
    )
    signature_request_id = str(create_result.get("signature_request_id", "")).strip()
    wait_events: list[dict[str, str]] = []
    signature_state = str(create_result.get("status", "UNKNOWN")).strip()
    if signature_request_id:
        signature_state, signature_wait_events = poll_signature_request_until_not_unsigned_fn(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=max(5, int(signature_wait_timeout_seconds)),
            warning_interval_seconds=max(5, int(signature_wait_warning_interval_seconds)),
        )
        wait_events.extend(signature_wait_events)
    return {
        "known_offer_markers": known_offer_markers,
        "offer_request_started_at": offer_request_started_at,
        "signature_request_id": signature_request_id,
        "signature_state": signature_state,
        "wait_events": wait_events,
        "expires_at": expires_at,
        "offer_amount": offer_amount,
        "request_amount": request_amount,
        "side": side,
    }


def cloud_wallet_wait_offer_artifact_phase(
    *,
    wallet: CloudWalletAdapter,
    known_markers: set[str],
    offer_request_started_at: dt.datetime,
    signature_request_id: str = "",
    timeout_seconds: int = 15 * 60,
    poll_offer_artifact_until_available_fn: collections.abc.Callable[..., str] | None = None,
    poll_offer_artifact_by_signature_request_fn: collections.abc.Callable[..., str] | None = None,
) -> str:
    if poll_offer_artifact_until_available_fn is None:
        poll_offer_artifact_until_available_fn = poll_offer_artifact_until_available
    if poll_offer_artifact_by_signature_request_fn is None:
        poll_offer_artifact_by_signature_request_fn = poll_offer_artifact_by_signature_request
    strict_timeout = max(15, int(timeout_seconds))
    if signature_request_id:
        try:
            return poll_offer_artifact_by_signature_request_fn(
                wallet=wallet,
                signature_request_id=signature_request_id,
                known_markers=known_markers,
                timeout_seconds=strict_timeout,
                min_created_at=offer_request_started_at,
            )
        except RuntimeError:
            # Signature-request scoped lookup is preferred when supported, but
            # not all test stubs or adapter variants implement this path.
            # Fall back to generic wallet offer polling in those cases.
            pass
    else:
        try:
            return poll_offer_artifact_until_available_fn(
                wallet=wallet,
                known_markers=known_markers,
                timeout_seconds=strict_timeout,
                min_created_at=offer_request_started_at,
                require_open_state=False,
                states=("OPEN", "PENDING"),
                prefer_newest=True,
            )
        except RuntimeError as exc:
            if str(exc) != "cloud_wallet_offer_artifact_timeout":
                raise
    extended_timeout = max(45, strict_timeout * 3)
    if signature_request_id:
        try:
            return poll_offer_artifact_by_signature_request_fn(
                wallet=wallet,
                signature_request_id=signature_request_id,
                known_markers=known_markers,
                timeout_seconds=int(extended_timeout),
                min_created_at=offer_request_started_at,
            )
        except RuntimeError:
            pass
    try:
        return poll_offer_artifact_until_available_fn(
            wallet=wallet,
            known_markers=known_markers,
            timeout_seconds=int(extended_timeout),
            min_created_at=offer_request_started_at,
            require_open_state=False,
            states=("OPEN", "PENDING"),
            prefer_newest=True,
        )
    except RuntimeError as retry_exc:
        if str(retry_exc) != "cloud_wallet_offer_artifact_timeout":
            raise
    return poll_offer_artifact_until_available_fn(
        wallet=wallet,
        known_markers=known_markers,
        timeout_seconds=15,
        min_created_at=offer_request_started_at,
        require_open_state=False,
        states=None,
        prefer_newest=False,
    )


def cloud_wallet_post_offer_phase(
    *,
    publish_venue: str,
    dexie: DexieAdapter | None,
    splash: SplashAdapter | None,
    offer_text: str,
    drop_only: bool,
    claim_rewards: bool,
    market: Any,
    expected_offered_asset_id: str,
    expected_offered_symbol: str,
    expected_requested_asset_id: str,
    expected_requested_symbol: str,
    post_dexie_offer_with_invalid_offer_retry_fn: collections.abc.Callable[..., dict[str, Any]]
    | None = None,
    verify_dexie_offer_visible_by_id_fn: collections.abc.Callable[..., str | None] | None = None,
    sleep_fn: collections.abc.Callable[[float], None] | None = None,
) -> dict[str, Any]:
    _ = market
    if post_dexie_offer_with_invalid_offer_retry_fn is None:
        post_dexie_offer_with_invalid_offer_retry_fn = post_dexie_offer_with_invalid_offer_retry
    if verify_dexie_offer_visible_by_id_fn is None:
        verify_dexie_offer_visible_by_id_fn = verify_dexie_offer_visible_by_id
    if sleep_fn is None:
        sleep_fn = time.sleep
    if publish_venue == "dexie":
        assert dexie is not None
        last_result: dict[str, Any] = {}
        last_visibility_error = ""
        for attempt in range(1, _DEXIE_VISIBILITY_POST_MAX_ATTEMPTS + 1):
            result = post_dexie_offer_with_invalid_offer_retry_fn(
                dexie=dexie,
                offer_text=offer_text,
                drop_only=drop_only,
                claim_rewards=claim_rewards,
            )
            last_result = dict(result)
            if not bool(result.get("success", False)):
                return result
            posted_offer_id = str(result.get("id", "")).strip()
            visibility_error = verify_dexie_offer_visible_by_id_fn(
                dexie=dexie,
                offer_id=posted_offer_id,
                expected_offered_asset_id=str(expected_offered_asset_id),
                expected_offered_symbol=str(expected_offered_symbol),
                expected_requested_asset_id=str(expected_requested_asset_id),
                expected_requested_symbol=str(expected_requested_symbol),
            )
            if not visibility_error:
                return result
            last_visibility_error = str(visibility_error)
            if not is_transient_dexie_visibility_404_error(last_visibility_error):
                return {
                    **result,
                    "success": False,
                    "error": last_visibility_error,
                }
            if attempt < _DEXIE_VISIBILITY_POST_MAX_ATTEMPTS:
                sleep_fn(_DEXIE_VISIBILITY_POST_DELAY_SECONDS)
        return {
            **last_result,
            "success": False,
            "error": (last_visibility_error or "dexie_offer_not_visible_after_publish"),
        }
    assert splash is not None
    return splash.post_offer(offer_text)


def build_and_post_offer_cloud_wallet(
    *,
    program: Any,
    market: Any,
    size_base_units: int,
    repeat: int,
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    quote_price: float,
    dry_run: bool,
    action_side: str = "sell",
    offer_artifact_timeout_seconds: int = 15 * 60,
    wallet_factory: collections.abc.Callable[[Any], CloudWalletAdapter] | None = None,
    dexie_adapter_cls: type[DexieAdapter] = DexieAdapter,
    splash_adapter_cls: type[SplashAdapter] = SplashAdapter,
    initialize_manager_file_logging_fn: collections.abc.Callable[..., None] | None = None,
    recent_market_resolved_asset_id_hints_fn: collections.abc.Callable[
        ..., tuple[str | None, str | None]
    ]
    | None = None,
    resolve_cloud_wallet_offer_asset_ids_fn: collections.abc.Callable[..., tuple[str, str]]
    | None = None,
    resolve_maker_offer_fee_fn: collections.abc.Callable[..., tuple[int, str]] | None = None,
    resolve_offer_expiry_for_market_fn: collections.abc.Callable[..., tuple[str, int]]
    | None = None,
    ensure_offer_bootstrap_denominations_fn: collections.abc.Callable[..., dict[str, Any]]
    | None = None,
    cloud_wallet_create_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    cloud_wallet_wait_offer_artifact_phase_fn: collections.abc.Callable[..., str] | None = None,
    log_signed_offer_artifact_fn: collections.abc.Callable[..., None] | None = None,
    verify_offer_text_for_dexie_fn: collections.abc.Callable[[str], str | None] | None = None,
    cloud_wallet_post_offer_phase_fn: collections.abc.Callable[..., dict[str, Any]] | None = None,
    dexie_offer_view_url_fn: collections.abc.Callable[..., str] | None = None,
) -> tuple[int, dict[str, Any]]:
    if wallet_factory is None:
        wallet_factory = new_cloud_wallet_adapter
    if initialize_manager_file_logging_fn is None:
        initialize_manager_file_logging_fn = initialize_manager_file_logging
    if recent_market_resolved_asset_id_hints_fn is None:
        recent_market_resolved_asset_id_hints_fn = recent_market_resolved_asset_id_hints
    if resolve_cloud_wallet_offer_asset_ids_fn is None:
        resolve_cloud_wallet_offer_asset_ids_fn = resolve_cloud_wallet_offer_asset_ids
    if resolve_maker_offer_fee_fn is None:
        resolve_maker_offer_fee_fn = resolve_maker_offer_fee
    if resolve_offer_expiry_for_market_fn is None:
        resolve_offer_expiry_for_market_fn = resolve_offer_expiry_for_market
    if ensure_offer_bootstrap_denominations_fn is None:
        ensure_offer_bootstrap_denominations_fn = ensure_offer_bootstrap_denominations
    if cloud_wallet_create_offer_phase_fn is None:
        cloud_wallet_create_offer_phase_fn = cloud_wallet_create_offer_phase
    if cloud_wallet_wait_offer_artifact_phase_fn is None:
        cloud_wallet_wait_offer_artifact_phase_fn = cloud_wallet_wait_offer_artifact_phase
    if log_signed_offer_artifact_fn is None:
        log_signed_offer_artifact_fn = log_signed_offer_artifact
    if verify_offer_text_for_dexie_fn is None:
        verify_offer_text_for_dexie_fn = verify_offer_text_for_dexie
    if cloud_wallet_post_offer_phase_fn is None:
        cloud_wallet_post_offer_phase_fn = cloud_wallet_post_offer_phase
    if dexie_offer_view_url_fn is None:
        dexie_offer_view_url_fn = dexie_offer_view_url

    side = normalize_offer_side(action_side)
    bootstrap_signature_wait_timeout_seconds = int(
        program.runtime_cloud_wallet_bootstrap_signature_wait_timeout_seconds
    )
    bootstrap_signature_warning_interval_seconds = int(
        program.runtime_cloud_wallet_bootstrap_signature_warning_interval_seconds
    )
    bootstrap_wait_timeout_seconds = int(
        program.runtime_cloud_wallet_bootstrap_wait_timeout_seconds
    )
    bootstrap_wait_mempool_warning_seconds = int(
        program.runtime_cloud_wallet_bootstrap_wait_mempool_warning_seconds
    )
    bootstrap_wait_confirmation_warning_seconds = int(
        program.runtime_cloud_wallet_bootstrap_wait_confirmation_warning_seconds
    )
    create_signature_wait_timeout_seconds = int(
        program.runtime_cloud_wallet_create_signature_wait_timeout_seconds
    )
    create_signature_warning_interval_seconds = int(
        program.runtime_cloud_wallet_create_signature_warning_interval_seconds
    )
    initialize_manager_file_logging_fn(
        program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
    )
    wallet = wallet_factory(program)
    cfg_base_global = str(getattr(market, "cloud_wallet_base_global_id", "")).strip()
    cfg_quote_global = str(getattr(market, "cloud_wallet_quote_global_id", "")).strip()
    db_base_hint, db_quote_hint = recent_market_resolved_asset_id_hints_fn(
        program_home_dir=str(program.home_dir),
        market_id=str(market.market_id),
    )
    base_global_hint = cfg_base_global or db_base_hint
    quote_global_hint = cfg_quote_global or db_quote_hint
    resolved_base_asset_id, resolved_quote_asset_id = resolve_cloud_wallet_offer_asset_ids_fn(
        wallet=wallet,
        base_asset_id=str(market.base_asset),
        quote_asset_id=str(market.quote_asset),
        base_symbol_hint=str(getattr(market, "base_symbol", "") or ""),
        quote_symbol_hint=str(getattr(market, "quote_asset", "") or ""),
        base_global_id_hint=base_global_hint,
        quote_global_id_hint=quote_global_hint,
        program_home_dir=str(program.home_dir),
    )
    db_path = (Path(program.home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()
    store = SqliteStore(db_path)
    post_results: list[dict] = []
    built_offers_preview: list[dict[str, str]] = []
    bootstrap_actions: list[dict[str, Any]] = []
    publish_failures = 0
    offer_fee_mojos, offer_fee_source = resolve_maker_offer_fee_fn(network=program.app_network)
    expiry_unit, expiry_value = resolve_offer_expiry_for_market_fn(market)
    dexie = (
        dexie_adapter_cls(dexie_base_url) if (not dry_run and publish_venue == "dexie") else None
    )
    splash = (
        splash_adapter_cls(splash_base_url) if (not dry_run and publish_venue == "splash") else None
    )

    for _ in range(repeat):
        _t: dict[str, int | None] = {
            "started": int(time.monotonic() * 1000),
            "create_phase_ms": None,
            "artifact_wait_ms": None,
            "create_total_ms": None,
            "publish_ms": None,
        }

        def _timing_payload(_t: dict[str, int | None] = _t) -> dict[str, int | None]:
            return {
                "create_phase_ms": _t["create_phase_ms"],
                "artifact_wait_ms": _t["artifact_wait_ms"],
                "create_total_ms": _t["create_total_ms"],
                "publish_ms": _t["publish_ms"],
                "total_ms": int(time.monotonic() * 1000) - int(_t["started"] or 0),
            }

        bootstrap_result: dict[str, Any] = {"status": "skipped", "reason": "dry_run"}
        if dry_run:
            bootstrap_actions.append(dict(bootstrap_result))
        else:
            bootstrap_result = ensure_offer_bootstrap_denominations_fn(
                program=program,
                market=market,
                wallet=wallet,
                resolved_base_asset_id=resolved_base_asset_id,
                resolved_quote_asset_id=resolved_quote_asset_id,
                quote_price=float(quote_price),
                action_side=side,
                bootstrap_signature_wait_timeout_seconds=bootstrap_signature_wait_timeout_seconds,
                bootstrap_signature_warning_interval_seconds=bootstrap_signature_warning_interval_seconds,
                bootstrap_wait_timeout_seconds=bootstrap_wait_timeout_seconds,
                bootstrap_wait_mempool_warning_seconds=bootstrap_wait_mempool_warning_seconds,
                bootstrap_wait_confirmation_warning_seconds=bootstrap_wait_confirmation_warning_seconds,
            )
            bootstrap_actions.append(bootstrap_result)
            bootstrap_status = str(bootstrap_result.get("status", "")).strip().lower()
            bootstrap_reason = (
                str(bootstrap_result.get("reason", "")).strip() or "bootstrap_precheck_failed"
            )
            bootstrap_ready = bool(bootstrap_result.get("ready", False))
            if bootstrap_status == "failed":
                if not bootstrap_result.get("fallback_to_cloud_wallet_offer_split"):
                    post_results.append(
                        {
                            "venue": publish_venue,
                            "result": {
                                "success": False,
                                "error": f"bootstrap_failed:{bootstrap_reason}",
                                "bootstrap": bootstrap_result,
                                "timing_ms": _timing_payload(),
                            },
                        }
                    )
                    publish_failures += 1
                    continue
            if bootstrap_status == "executed" and not bootstrap_ready:
                post_results.append(
                    {
                        "venue": publish_venue,
                        "result": {
                            "success": False,
                            "error": f"bootstrap_pending:{bootstrap_reason}",
                            "bootstrap": bootstrap_result,
                            "timing_ms": _timing_payload(),
                        },
                    }
                )
                publish_failures += 1
                continue
            if bootstrap_status == "skipped" and bootstrap_reason != "already_ready":
                post_results.append(
                    {
                        "venue": publish_venue,
                        "result": {
                            "success": False,
                            "error": f"bootstrap_precheck_skipped:{bootstrap_reason}",
                            "bootstrap": bootstrap_result,
                            "timing_ms": _timing_payload(),
                        },
                    }
                )
                publish_failures += 1
                continue

        create_started = time.monotonic()
        try:
            create_phase = cloud_wallet_create_offer_phase_fn(
                wallet=wallet,
                market=market,
                size_base_units=size_base_units,
                quote_price=quote_price,
                resolved_base_asset_id=resolved_base_asset_id,
                resolved_quote_asset_id=resolved_quote_asset_id,
                offer_fee_mojos=offer_fee_mojos,
                split_input_coins_fee=0,
                expiry_unit=expiry_unit,
                expiry_value=expiry_value,
                action_side=side,
                signature_wait_timeout_seconds=create_signature_wait_timeout_seconds,
                signature_wait_warning_interval_seconds=create_signature_warning_interval_seconds,
            )
            _t["create_phase_ms"] = int((time.monotonic() - create_started) * 1000)
        except Exception as exc:
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {
                        "success": False,
                        "error": str(exc),
                        "timing_ms": _timing_payload(),
                    },
                }
            )
            publish_failures += 1
            continue
        signature_request_id = str(create_phase["signature_request_id"]).strip()
        signature_state = str(create_phase["signature_state"]).strip()
        wait_events = list(create_phase["wait_events"])
        expires_at = str(create_phase["expires_at"])
        offer_text = ""
        wait_started = time.monotonic()
        try:
            offer_text = cloud_wallet_wait_offer_artifact_phase_fn(
                wallet=wallet,
                known_markers=set(create_phase["known_offer_markers"]),
                offer_request_started_at=create_phase["offer_request_started_at"],
                signature_request_id=signature_request_id,
                timeout_seconds=int(offer_artifact_timeout_seconds),
            )
            _t["artifact_wait_ms"] = int((time.monotonic() - wait_started) * 1000)
            _t["create_total_ms"] = int((time.monotonic() - create_started) * 1000)
        except RuntimeError as exc:
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {
                        "success": False,
                        "error": str(exc),
                        "signature_request_id": signature_request_id,
                        "signature_state": signature_state,
                        "wait_events": wait_events,
                        "timing_ms": _timing_payload(),
                    },
                }
            )
            publish_failures += 1
            continue
        if not offer_text:
            publish_failures += 1
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {
                        "success": False,
                        "error": "cloud_wallet_offer_artifact_unavailable",
                        "signature_request_id": signature_request_id,
                        "signature_state": signature_state,
                        "wait_events": wait_events,
                        "timing_ms": _timing_payload(),
                    },
                }
            )
            continue

        log_signed_offer_artifact_fn(
            offer_text=offer_text,
            ticker=str(market.base_symbol),
            amount=int(size_base_units),
            trading_pair=f"{market.base_symbol}:{market.quote_asset}",
            expiry=str(expires_at),
        )
        verify_error = verify_offer_text_for_dexie_fn(offer_text)
        if verify_error:
            publish_failures += 1
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {
                        "success": False,
                        "error": verify_error,
                        "timing_ms": _timing_payload(),
                    },
                }
            )
            continue
        if dry_run:
            built_offers_preview.append(
                {
                    "offer_prefix": offer_text[:24],
                    "offer_length": str(len(offer_text)),
                }
            )
            continue

        publish_started = time.monotonic()
        result = cloud_wallet_post_offer_phase_fn(
            publish_venue=publish_venue,
            dexie=dexie,
            splash=splash,
            offer_text=offer_text,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
            market=market,
            expected_offered_asset_id=(
                str(resolved_quote_asset_id)
                if str(create_phase.get("side", "sell")).strip().lower() == "buy"
                else str(resolved_base_asset_id)
            ),
            expected_offered_symbol=(
                str(getattr(market, "quote_asset", ""))
                if str(create_phase.get("side", "sell")).strip().lower() == "buy"
                else str(getattr(market, "base_symbol", ""))
            ),
            expected_requested_asset_id=(
                str(resolved_base_asset_id)
                if str(create_phase.get("side", "sell")).strip().lower() == "buy"
                else str(resolved_quote_asset_id)
            ),
            expected_requested_symbol=(
                str(getattr(market, "base_symbol", ""))
                if str(create_phase.get("side", "sell")).strip().lower() == "buy"
                else str(getattr(market, "quote_asset", ""))
            ),
        )
        _t["publish_ms"] = int((time.monotonic() - publish_started) * 1000)
        if result.get("success") is False:
            publish_failures += 1
        offer_id = str(result.get("id", "")).strip()
        result_payload = {
            **result,
            "signature_request_id": signature_request_id,
            "signature_state": signature_state,
            "wait_events": wait_events,
            "timing_ms": _timing_payload(),
        }
        if publish_venue == "dexie" and offer_id:
            result_payload["offer_view_url"] = dexie_offer_view_url_fn(
                dexie_base_url=dexie_base_url,
                offer_id=offer_id,
            )
        if offer_id and bool(result.get("success", False)):
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=str(market.market_id),
                state=OfferLifecycleState.OPEN.value,
                last_seen_status=None,
            )
            store.add_audit_event(
                "strategy_offer_execution",
                {
                    "market_id": str(market.market_id),
                    "planned_count": 1,
                    "executed_count": 1,
                    "items": [
                        {
                            "size": int(size_base_units),
                            "side": side,
                            "status": "executed",
                            "reason": f"{publish_venue}_post_success",
                            "offer_id": offer_id,
                            "attempts": 1,
                        }
                    ],
                    "venue": publish_venue,
                    "signature_request_id": signature_request_id,
                    "signature_state": signature_state,
                    "resolved_base_asset_id": resolved_base_asset_id,
                    "resolved_quote_asset_id": resolved_quote_asset_id,
                },
                market_id=str(market.market_id),
            )
        post_results.append({"venue": publish_venue, "result": result_payload})

    payload: dict[str, Any] = {
        "market_id": market.market_id,
        "pair": f"{market.base_asset}:{market.quote_asset}",
        "resolved_base_asset_id": resolved_base_asset_id,
        "resolved_quote_asset_id": resolved_quote_asset_id,
        "network": program.app_network,
        "size_base_units": size_base_units,
        "repeat": repeat,
        "publish_venue": publish_venue,
        "dexie_base_url": dexie_base_url,
        "splash_base_url": splash_base_url if publish_venue == "splash" else None,
        "drop_only": drop_only,
        "claim_rewards": claim_rewards,
        "dry_run": bool(dry_run),
        "publish_attempts": len(post_results),
        "publish_failures": publish_failures,
        "built_offers_preview": built_offers_preview,
        "bootstrap_actions": bootstrap_actions,
        "results": post_results,
        "offer_fee_mojos": offer_fee_mojos,
        "offer_fee_source": offer_fee_source,
    }
    print(_format_json_output(payload))
    store.close()
    return (0 if publish_failures == 0 else 2), payload
