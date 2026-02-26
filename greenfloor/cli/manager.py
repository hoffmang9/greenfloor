from __future__ import annotations

import argparse
import collections.abc
import datetime as dt
import importlib
import json
import math
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

import yaml

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter, extract_coinset_tx_ids_from_offer_payload
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.cli.offer_builder_sdk import build_offer_text
from greenfloor.config.io import load_markets_config, load_program_config, load_yaml
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.keys.onboarding import (
    KeyOnboardingSelection,
    determine_onboarding_branch,
    discover_chia_keys,
    save_key_onboarding_selection,
)
from greenfloor.keys.router import resolve_market_key
from greenfloor.storage.sqlite import SqliteStore

_TEST_PHASE_OFFER_EXPIRY_MINUTES = 10


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
        conditions_fn = getattr(coin_spend, "conditions", None)
        if not callable(conditions_fn):
            continue
        conditions = conditions_fn() or []
        if not isinstance(conditions, list):
            continue
        for condition in conditions:
            if _condition_has_offer_expiration(condition):
                return True
    return False


def _verify_offer_text_for_dexie(offer_text: str) -> str | None:
    try:
        native = importlib.import_module("greenfloor_native")
    except Exception:
        native = None
    else:
        try:
            native.validate_offer(offer_text)
            return None
        except Exception as exc:
            return f"wallet_sdk_offer_validate_failed:{exc}"

    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception as exc:
        return f"wallet_sdk_import_error:{exc}"
    try:
        validate_offer = getattr(sdk, "validate_offer", None)
        if callable(validate_offer):
            validate_offer(offer_text)
        else:
            verify_offer = getattr(sdk, "verify_offer", None)
            if not callable(verify_offer):
                return "wallet_sdk_validate_offer_unavailable"
            if not bool(verify_offer(offer_text)):
                return "wallet_sdk_offer_verify_false"

        if not _offer_has_expiration_condition(sdk, offer_text):
            return "wallet_sdk_offer_missing_expiration"
    except Exception as exc:
        return f"wallet_sdk_offer_validate_failed:{exc}"
    return None


def _default_program_config_path() -> str:
    home_default = Path("~/.greenfloor/config/program.yaml").expanduser()
    if home_default.exists():
        return str(home_default)
    return "config/program.yaml"


def _default_markets_config_path() -> str:
    home_default = Path("~/.greenfloor/config/markets.yaml").expanduser()
    if home_default.exists():
        return str(home_default)
    return "config/markets.yaml"


_JSON_OUTPUT_COMPACT = False


def _format_json_output(payload: object) -> str:
    if _JSON_OUTPUT_COMPACT:
        return json.dumps(payload, separators=(",", ":"))
    return json.dumps(payload, indent=2)


class _CoinsetFeeLookupPreflightError(RuntimeError):
    def __init__(
        self,
        *,
        failure_kind: str,
        detail: str,
        diagnostics: dict[str, str],
    ) -> None:
        self.failure_kind = failure_kind
        self.detail = detail
        self.diagnostics = diagnostics
        super().__init__(f"{failure_kind}:{detail}")


def _require_cloud_wallet_config(program) -> CloudWalletConfig:
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
    )


def _new_cloud_wallet_adapter(program) -> CloudWalletAdapter:
    return CloudWalletAdapter(_require_cloud_wallet_config(program))


def _canonical_is_xch(asset_id: str) -> bool:
    value = asset_id.strip().lower()
    return value in {"xch", "txch"}


def _canonical_is_cloud_global_id(asset_id: str) -> bool:
    return asset_id.strip().startswith("Asset_")


def _is_hex_asset_id(value: str) -> bool:
    raw = value.strip().lower()
    return len(raw) == 64 and all(c in "0123456789abcdef" for c in raw)


def _normalize_label(value: str) -> str:
    return "".join(ch for ch in value.strip().lower() if ch.isalnum())


def _label_tokens(value: str) -> list[str]:
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


def _labels_match(left: str, right: str) -> bool:
    a = _normalize_label(left)
    b = _normalize_label(right)
    if not a or not b:
        return False
    if a == b:
        return True
    # Keep fuzzy matching conservative; require meaningful overlap length.
    if len(a) >= 5 and a in b:
        return True
    if len(b) >= 5 and b in a:
        return True
    left_tokens = {token for token in _label_tokens(left) if len(token) >= 3}
    right_tokens = {token for token in _label_tokens(right) if len(token) >= 3}
    if left_tokens and right_tokens and len(left_tokens & right_tokens) >= 2:
        return True
    return False


def _wallet_label_matches_asset_ref(
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
        if _labels_match(display_name, target) or _labels_match(symbol, target):
            matches.append(asset_id)
    return sorted(set(matches))


def _local_catalog_label_hints_for_asset_id(*, canonical_asset_id: str) -> list[str]:
    """Best-effort local label hints when Dexie metadata is unavailable."""
    canonical = canonical_asset_id.strip().lower()
    if not canonical:
        return []
    repo_root = Path(__file__).resolve().parents[2]
    markets_path = repo_root / "config" / "markets.yaml"
    if not markets_path.exists():
        return []
    try:
        payload = load_yaml(markets_path)
    except Exception:
        return []
    hints: list[str] = []
    assets_rows = payload.get("assets") if isinstance(payload, dict) else None
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
    markets_rows = payload.get("markets") if isinstance(payload, dict) else None
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


def _dexie_lookup_token_for_cat_id(*, canonical_cat_id_hex: str, network: str) -> dict | None:
    base_url = _resolve_dexie_base_url(network, None)
    target = canonical_cat_id_hex.strip().lower()
    if not target:
        return None

    def _fetch_json(url: str) -> object | None:
        req = urllib.request.Request(
            url,
            method="GET",
            headers={
                "Accept": "application/json",
                "User-Agent": "greenfloor/0.1",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=20) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except Exception:
            return None

    def _row_matches_target(row: dict, *, include_ticker_split: bool = False) -> bool:
        candidates = {
            str(row.get("assetId", "")).strip().lower(),
            str(row.get("asset_id", "")).strip().lower(),
            str(row.get("id", "")).strip().lower(),
            str(row.get("tokenId", "")).strip().lower(),
            str(row.get("token_id", "")).strip().lower(),
            str(row.get("base_currency", "")).strip().lower(),
            str(row.get("target_currency", "")).strip().lower(),
        }
        ticker_id = str(row.get("ticker_id", "")).strip().lower()
        if ticker_id:
            candidates.add(ticker_id)
            if include_ticker_split and "_" in ticker_id:
                base, quote = ticker_id.split("_", 1)
                candidates.add(base)
                candidates.add(quote)
        return target in candidates

    # Primary: swap token metadata.
    tokens_payload = _fetch_json(f"{base_url}/v1/swap/tokens")
    token_rows: list[dict] = []
    if isinstance(tokens_payload, list):
        token_rows = [row for row in tokens_payload if isinstance(row, dict)]
    elif isinstance(tokens_payload, dict):
        tokens = tokens_payload.get("tokens")
        if isinstance(tokens, list):
            token_rows = [row for row in tokens if isinstance(row, dict)]
    for row in token_rows:
        if _row_matches_target(row):
            return row

    # Fallback: v3 ticker metadata often includes CAT tails not present in
    # swap token listing (for example CARBON22 on some Dexie snapshots).
    tickers_payload = _fetch_json(f"{base_url}/v3/prices/tickers")
    ticker_rows: list[dict] = []
    if isinstance(tickers_payload, list):
        ticker_rows = [row for row in tickers_payload if isinstance(row, dict)]
    elif isinstance(tickers_payload, dict):
        tickers = tickers_payload.get("tickers")
        if isinstance(tickers, list):
            ticker_rows = [row for row in tickers if isinstance(row, dict)]
    for row in ticker_rows:
        if _row_matches_target(row, include_ticker_split=True):
            return row
    return None


def _dexie_lookup_token_for_symbol(*, asset_ref: str, network: str) -> dict | None:
    base_url = _resolve_dexie_base_url(network, None)
    req = urllib.request.Request(
        f"{base_url}/v1/swap/tokens",
        method="GET",
        headers={
            "Accept": "application/json",
            "User-Agent": "greenfloor/0.1",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            payload = json.loads(resp.read().decode("utf-8"))
    except Exception:
        return None

    rows: list[dict] = []
    if isinstance(payload, list):
        rows = [row for row in payload if isinstance(row, dict)]
    elif isinstance(payload, dict):
        tokens = payload.get("tokens")
        if isinstance(tokens, list):
            rows = [row for row in tokens if isinstance(row, dict)]

    target = asset_ref.strip()
    for row in rows:
        if _labels_match(str(row.get("code", "")), target):
            return row
        if _labels_match(str(row.get("name", "")), target):
            return row
        if _labels_match(str(row.get("id", "")), target):
            return row
    return None


def _resolve_cloud_wallet_asset_id(
    *,
    wallet: CloudWalletAdapter,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
) -> str:
    raw = canonical_asset_id.strip()
    if not raw:
        raise ValueError("asset_id must be non-empty")
    if _canonical_is_cloud_global_id(raw):
        return raw
    if not hasattr(wallet, "_graphql"):
        # Test doubles and alternate adapters may not expose raw GraphQL.
        return raw

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
    edges = assets_payload.get("edges") or []

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
        elif asset_type in {"CAT2", "CAT", "TOKEN"}:
            cat_assets.append(
                {
                    "asset_id": asset_global_id,
                    "display_name": display_name,
                    "symbol": symbol,
                }
            )

    if _canonical_is_xch(raw):
        if len(crypto_asset_ids) == 1:
            return crypto_asset_ids[0]
        if len(crypto_asset_ids) == 0:
            raise RuntimeError("cloud_wallet_asset_resolution_failed:no_crypto_asset_found_for_xch")
        raise RuntimeError("cloud_wallet_asset_resolution_failed:ambiguous_crypto_asset_for_xch")

    if not cat_assets:
        raise RuntimeError(
            f"cloud_wallet_asset_resolution_failed:no_wallet_cat_asset_candidates_for:{raw}"
        )

    canonical_hex = raw.lower()
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
    dexie_token = _dexie_lookup_token_for_cat_id(
        canonical_cat_id_hex=canonical_hex,
        network=wallet.network,
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
    if dexie_token is None:
        raise RuntimeError(
            f"cloud_wallet_asset_resolution_failed:dexie_cat_metadata_not_found_for:{raw}"
        )
    if len(cat_assets) == 1:
        return cat_assets[0]["asset_id"]
    raise RuntimeError(f"cloud_wallet_asset_resolution_failed:unmatched_wallet_cat_asset_for:{raw}")


def _resolve_cloud_wallet_offer_asset_ids(
    *,
    wallet: CloudWalletAdapter,
    base_asset_id: str,
    quote_asset_id: str,
    base_symbol_hint: str | None = None,
    quote_symbol_hint: str | None = None,
) -> tuple[str, str]:
    resolved_base = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=base_asset_id,
        symbol_hint=(base_symbol_hint or "").strip() or str(base_asset_id).strip(),
    )
    resolved_quote = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=quote_asset_id,
        symbol_hint=(quote_symbol_hint or "").strip() or str(quote_asset_id).strip(),
    )
    if (
        resolved_base == resolved_quote
        and not _canonical_is_xch(base_asset_id)
        and not _canonical_is_xch(quote_asset_id)
        and not _canonical_is_cloud_global_id(base_asset_id)
        and not _canonical_is_cloud_global_id(quote_asset_id)
    ):
        raise RuntimeError(
            "cloud_wallet_asset_resolution_failed:resolved_assets_collide_for_non_xch_pair"
        )
    return resolved_base, resolved_quote


def _parse_iso8601(value: str) -> dt.datetime | None:
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


def _offer_markers(offers: list[dict]) -> set[str]:
    markers: set[str] = set()
    for offer in offers:
        offer_id = str(offer.get("offerId", "")).strip()
        if offer_id:
            markers.add(f"id:{offer_id}")
        bech32 = str(offer.get("bech32", "")).strip()
        if bech32:
            markers.add(f"bech32:{bech32}")
    return markers


def _pick_new_offer_artifact(*, offers: list[dict], known_markers: set[str]) -> str:
    candidates: list[tuple[dt.datetime, str]] = []
    for offer in offers:
        bech32 = str(offer.get("bech32", "")).strip()
        if not bech32.startswith("offer1"):
            continue
        offer_id = str(offer.get("offerId", "")).strip()
        markers = {f"bech32:{bech32}"}
        if offer_id:
            markers.add(f"id:{offer_id}")
        if markers.issubset(known_markers):
            continue
        expires_at = _parse_iso8601(str(offer.get("expiresAt", "")).strip())
        candidates.append((expires_at or dt.datetime.min.replace(tzinfo=dt.UTC), bech32))
    if not candidates:
        return ""
    candidates.sort(key=lambda row: row[0], reverse=True)
    return candidates[0][1]


def _safe_int(value: object) -> int | None:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def _call_with_moderate_retry(
    *,
    action: str,
    call: collections.abc.Callable[[], Any],
    elapsed_seconds: int = 0,
    events: list[dict[str, str]] | None = None,
    max_attempts: int = 4,
):
    attempt = 0
    sleep_seconds = 0.5
    while True:
        try:
            return call()
        except Exception as exc:
            attempt += 1
            if attempt >= max_attempts:
                raise RuntimeError(f"{action}_retry_exhausted:{exc}") from exc
            if events is not None:
                events.append(
                    {
                        "event": "poll_retry",
                        "action": action,
                        "attempt": str(attempt),
                        "elapsed_seconds": str(elapsed_seconds),
                        "wait_reason": "transient_poll_failure",
                        "error": str(exc),
                    }
                )
            time.sleep(sleep_seconds)
            sleep_seconds = min(8.0, sleep_seconds * 2.0)


def _coinset_coin_url(*, coin_name: str, network: str = "mainnet") -> str:
    base = (
        "https://testnet11.coinset.org"
        if network.strip().lower() in {"testnet", "testnet11"}
        else "https://coinset.org"
    )
    return f"{base}/coin/{coin_name.strip()}"


def _coinset_reconcile_coin_state(*, network: str, coin_name: str) -> dict[str, str]:
    adapter = CoinsetAdapter(None, network=network)
    try:
        record = _call_with_moderate_retry(
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
    state = _call_with_moderate_retry(
        action="coinset_get_blockchain_state",
        call=adapter.get_blockchain_state,
    )
    if not isinstance(state, dict):
        return None
    candidates = [
        state.get("peak_height"),
        state.get("peakHeight"),
    ]
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


def _poll_offer_artifact_until_available(
    *,
    wallet: CloudWalletAdapter,
    known_markers: set[str],
    timeout_seconds: int,
) -> str:
    start = time.monotonic()
    sleep_seconds = 2.0
    while True:
        elapsed = int(time.monotonic() - start)
        wallet_payload = _call_with_moderate_retry(
            action="wallet_get_wallet",
            call=wallet.get_wallet,
            elapsed_seconds=elapsed,
        )
        offers = wallet_payload.get("offers", [])
        if isinstance(offers, list):
            offer_text = _pick_new_offer_artifact(offers=offers, known_markers=known_markers)
            if offer_text:
                return offer_text
        if elapsed >= timeout_seconds:
            raise RuntimeError("cloud_wallet_offer_artifact_timeout")
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _coinset_base_url(*, network: str) -> str:
    base = os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()
    if not base:
        return ""
    network_l = network.strip().lower()
    if network_l in {"testnet", "testnet11"}:
        allow_mainnet = os.getenv("GREENFLOOR_ALLOW_MAINNET_COINSET_FOR_TESTNET11", "").strip()
        if (
            "coinset.org" in base
            and "testnet11.api.coinset.org" not in base
            and allow_mainnet != "1"
        ):
            raise RuntimeError("coinset_base_url_mainnet_not_allowed_for_testnet11")
    return base


def _coinset_adapter(*, network: str) -> CoinsetAdapter:
    base_url = _coinset_base_url(network=network)
    require_testnet11 = network.strip().lower() in {"testnet", "testnet11"}
    try:
        return CoinsetAdapter(
            base_url or None, network=network, require_testnet11=require_testnet11
        )
    except TypeError as exc:
        # Test doubles in deterministic unit tests may not accept the newer kwarg.
        if "require_testnet11" not in str(exc):
            raise
        return CoinsetAdapter(base_url or None, network=network)


def _coinset_fee_lookup_preflight(*, network: str) -> dict[str, str]:
    try:
        coinset = _coinset_adapter(network=network)
    except Exception as exc:
        raise _CoinsetFeeLookupPreflightError(
            failure_kind="endpoint_validation_failed",
            detail=str(exc),
            diagnostics={
                "coinset_network": network.strip().lower(),
                "coinset_base_url": os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip(),
            },
        ) from exc
    diagnostics = {
        "coinset_network": str(getattr(coinset, "network", network.strip().lower())),
        "coinset_base_url": str(
            getattr(coinset, "base_url", os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip())
        ),
    }
    try:
        payload = coinset.get_fee_estimate(target_times=[300, 600, 1200])
    except Exception as exc:
        raise _CoinsetFeeLookupPreflightError(
            failure_kind="endpoint_validation_failed",
            detail=str(exc),
            diagnostics=diagnostics,
        ) from exc

    if not bool(payload.get("success", False)):
        detail = str(
            payload.get("error")
            or payload.get("message")
            or payload.get("reason")
            or "coinset_fee_estimate_unsuccessful"
        )
        raise _CoinsetFeeLookupPreflightError(
            failure_kind="temporary_fee_advice_unavailable",
            detail=detail,
            diagnostics=diagnostics,
        )

    recommended = coinset.get_conservative_fee_estimate()
    if recommended is None:
        raise _CoinsetFeeLookupPreflightError(
            failure_kind="temporary_fee_advice_unavailable",
            detail="coinset_conservative_fee_unavailable",
            diagnostics=diagnostics,
        )
    diagnostics["recommended_fee_mojos"] = str(int(recommended))
    return diagnostics


def _resolve_operation_fee(
    *,
    role: str,
    network: str,
    minimum_fee_mojos: int = 0,
) -> tuple[int, str]:
    if role == "maker_create_offer":
        return 0, "maker_default_zero"
    if role != "taker_or_coin_operation":
        raise ValueError(f"unsupported fee role: {role}")
    if int(minimum_fee_mojos) < 0:
        raise ValueError("minimum_fee_mojos must be >= 0")

    minimum_fee = int(minimum_fee_mojos)
    max_attempts = int(os.getenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "4"))
    coinset = _coinset_adapter(network=network)
    for attempt in range(max_attempts):
        advised = None
        try:
            advised = coinset.get_conservative_fee_estimate()
        except Exception:
            advised = None
        if advised is not None:
            advised_fee = int(advised)
            if advised_fee < minimum_fee:
                return minimum_fee, "coinset_conservative_minimum_floor"
            return advised_fee, "coinset_conservative"
        if attempt < max_attempts - 1:
            sleep_seconds = min(8.0, 0.5 * (2**attempt))
            time.sleep(sleep_seconds)

    return minimum_fee, "config_minimum_fee_fallback"


def _resolve_taker_or_coin_operation_fee(
    *, network: str, minimum_fee_mojos: int = 0
) -> tuple[int, str]:
    _coinset_fee_lookup_preflight(network=network)
    return _resolve_operation_fee(
        role="taker_or_coin_operation",
        network=network,
        minimum_fee_mojos=minimum_fee_mojos,
    )


def _resolve_maker_offer_fee(*, network: str) -> tuple[int, str]:
    return _resolve_operation_fee(role="maker_create_offer", network=network)


def _poll_signature_request_until_not_unsigned(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    timeout_seconds: int,
    warning_interval_seconds: int,
) -> tuple[str, list[dict[str, str]]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    next_warning = warning_interval_seconds
    warning_count = 0
    next_heartbeat = 5
    sleep_seconds = 2.0
    while True:
        elapsed = int(time.monotonic() - start)
        status_payload = _call_with_moderate_retry(
            action="wallet_get_signature_request",
            call=lambda: wallet.get_signature_request(signature_request_id=signature_request_id),
            elapsed_seconds=elapsed,
            events=events,
        )
        status = str(status_payload.get("status", "")).strip().upper()
        if status and status != "UNSIGNED":
            # Keep terminal output readable when heartbeat dots were emitted.
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
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _wait_for_mempool_then_confirmation(
    *,
    wallet: CloudWalletAdapter,
    network: str,
    initial_coin_ids: set[str],
    mempool_warning_seconds: int,
    confirmation_warning_seconds: int,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    seen_pending = False
    next_heartbeat = 5
    sleep_seconds = 2.0
    next_mempool_warning = mempool_warning_seconds
    next_confirmation_warning = confirmation_warning_seconds
    while True:
        elapsed = int(time.monotonic() - start)
        coins = _call_with_moderate_retry(
            action="wallet_list_coins",
            call=lambda: wallet.list_coins(include_pending=True),
            elapsed_seconds=elapsed,
            events=events,
        )
        pending = [
            c
            for c in coins
            if str(c.get("state", "")).strip().upper() in {"PENDING", "MEMPOOL"}
            and str(c.get("id", "")).strip() not in initial_coin_ids
        ]
        confirmed = [
            c
            for c in coins
            if str(c.get("state", "")).strip().upper() not in {"PENDING", "MEMPOOL"}
            and str(c.get("id", "")).strip() not in initial_coin_ids
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
            # Keep terminal output readable when heartbeat dots were emitted.
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
        if not seen_pending and elapsed >= next_mempool_warning:
            events.append(
                {
                    "event": "mempool_wait_warning",
                    "elapsed_seconds": str(elapsed),
                }
            )
            next_mempool_warning += mempool_warning_seconds
        if seen_pending and elapsed >= next_confirmation_warning:
            events.append(
                {
                    "event": "confirmation_wait_warning",
                    "elapsed_seconds": str(elapsed),
                }
            )
            next_confirmation_warning += confirmation_warning_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _is_spendable_coin(coin: dict) -> bool:
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


def _coin_asset_id(coin: dict) -> str:
    asset_raw = coin.get("asset")
    if isinstance(asset_raw, dict):
        return str(asset_raw.get("id", "xch")).strip() or "xch"
    if isinstance(asset_raw, str):
        return asset_raw.strip() or "xch"
    return "xch"


def _evaluate_denomination_readiness(
    *,
    wallet: CloudWalletAdapter,
    asset_id: str,
    size_base_units: int,
    required_min_count: int | None = None,
    max_allowed_count: int | None = None,
) -> dict[str, int | bool | str]:
    coins = wallet.list_coins(include_pending=True)
    spendable = [
        c
        for c in coins
        if _is_spendable_coin(c)
        and _coin_asset_id(c).lower() == asset_id.strip().lower()
        and int(c.get("amount", 0)) == int(size_base_units)
    ]
    current_count = len(spendable)
    ready = True
    if required_min_count is not None:
        ready = current_count >= int(required_min_count)
    if max_allowed_count is not None:
        ready = ready and current_count <= int(max_allowed_count)
    return {
        "asset_id": asset_id,
        "size_base_units": int(size_base_units),
        "current_count": current_count,
        "required_min_count": int(required_min_count) if required_min_count is not None else -1,
        "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
        "ready": ready,
    }


def _as_wait_events(value: object) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    items: list[dict[str, str]] = []
    for row in value:
        if isinstance(row, dict):
            event = {str(k): str(v) for k, v in row.items()}
            items.append(event)
    return items


def _resolve_coin_global_ids(
    wallet_coins: list[dict], raw_coin_ids: list[str]
) -> tuple[list[str], list[str]]:
    """Map operator hex coin names (or Coin_* global IDs) to Cloud Wallet global IDs.

    Returns (resolved_ids, unresolved_ids).  Operators usually copy hex coin names
    from ``coins-list`` output; Cloud Wallet mutations require the ``Coin_*`` GraphQL
    global-ID form.  Direct ``Coin_*`` IDs are passed through unchanged for power users.
    """
    mapping: dict[str, str] = {}
    for coin in wallet_coins:
        global_id = str(coin.get("id", "")).strip()
        name = str(coin.get("name", "")).strip()
        if global_id:
            mapping[global_id] = global_id
        if name and global_id:
            mapping[name] = global_id
    resolved: list[str] = []
    unresolved: list[str] = []
    for raw in raw_coin_ids:
        token = str(raw).strip()
        mapped = mapping.get(token)
        if mapped:
            resolved.append(mapped)
        elif token.startswith("Coin_"):
            resolved.append(token)
        else:
            unresolved.append(token)
    return resolved, unresolved


# ---------------------------------------------------------------------------
# Shared coin-operation helpers
# ---------------------------------------------------------------------------


def _coin_op_base_payload(
    market: Any, selected_venue: str | None, wallet: CloudWalletAdapter
) -> dict[str, object]:
    return {
        "market_id": market.market_id,
        "pair": f"{market.base_symbol}:{market.quote_asset}",
        "venue": selected_venue,
        "vault_id": wallet.vault_id,
    }


def _resolve_coin_op_fee(
    *,
    network: str,
    minimum_fee_mojos: int,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
) -> tuple[int, str] | None:
    """Resolve fee for a coin operation.

    Returns ``(fee_mojos, fee_source)`` on success or ``None`` after printing
    a structured JSON error payload.
    """
    try:
        return _resolve_taker_or_coin_operation_fee(
            network=network,
            minimum_fee_mojos=minimum_fee_mojos,
        )
    except _CoinsetFeeLookupPreflightError as exc:
        operator_guidance = (
            "verify Coinset endpoint routing: unset GREENFLOOR_COINSET_BASE_URL to use "
            "network defaults, or set it to a valid endpoint for the active network"
            if exc.failure_kind == "endpoint_validation_failed"
            else "coinset fee advice is temporarily unavailable; retry shortly and verify Coinset fee endpoint health before resubmitting"
        )
        print(
            _format_json_output(
                {
                    **_coin_op_base_payload(market, selected_venue, wallet),
                    "waited": False,
                    "success": False,
                    "error": f"coinset_fee_preflight_failed:{exc.failure_kind}",
                    "coinset_fee_lookup": {
                        "status": "failed",
                        "failure_kind": exc.failure_kind,
                        "detail": exc.detail,
                        **exc.diagnostics,
                    },
                    "operator_guidance": operator_guidance,
                }
            )
        )
        return None
    except Exception as exc:
        print(
            _format_json_output(
                {
                    **_coin_op_base_payload(market, selected_venue, wallet),
                    "waited": False,
                    "success": False,
                    "error": f"fee_resolution_failed:{exc}",
                    "operator_guidance": (
                        "set coin_ops.minimum_fee_mojos in program config (can be 0) "
                        "or fix GREENFLOOR_COINSET_BASE_URL to a valid Coinset API endpoint"
                    ),
                }
            )
        )
        return None


def _coin_op_build_iteration_payload(
    *,
    wallet: CloudWalletAdapter,
    signature_request_id: str,
    initial_signature_state: str,
    no_wait: bool,
    network: str,
    existing_coin_ids: set[str],
    iteration: int,
    denomination_target: dict[str, Any] | None,
    readiness_asset_id: str,
    readiness_kwargs: dict[str, int],
) -> tuple[dict[str, object], dict[str, int | bool | str] | None]:
    """Poll signature, wait for confirmation, evaluate readiness."""
    wait_events: list[dict[str, str]] = []
    final_signature_state = initial_signature_state
    if not no_wait:
        final_signature_state, signature_events = _poll_signature_request_until_not_unsigned(
            wallet=wallet,
            signature_request_id=signature_request_id,
            timeout_seconds=15 * 60,
            warning_interval_seconds=10 * 60,
        )
        wait_events.extend(signature_events)
        wait_events.extend(
            _wait_for_mempool_then_confirmation(
                wallet=wallet,
                network=network,
                initial_coin_ids=existing_coin_ids,
                mempool_warning_seconds=5 * 60,
                confirmation_warning_seconds=15 * 60,
            )
        )
    iteration_payload: dict[str, object] = {
        "iteration": iteration,
        "signature_request_id": signature_request_id,
        "signature_state": final_signature_state,
        "waited": not no_wait,
        "wait_events": wait_events,
    }
    final_readiness = None
    if denomination_target is not None:
        final_readiness = _evaluate_denomination_readiness(
            wallet=wallet,
            asset_id=readiness_asset_id,
            size_base_units=int(denomination_target["size_base_units"]),
            **readiness_kwargs,
        )
        iteration_payload["denomination_readiness"] = final_readiness
    return iteration_payload, final_readiness


def _coin_op_should_stop(
    *,
    until_ready: bool,
    final_readiness: dict[str, int | bool | str] | None,
    coin_ids: list[str],
    iteration: int,
    max_iterations: int,
) -> tuple[bool, str]:
    """Return ``(should_break, stop_reason)`` for the iteration loop."""
    if not until_ready or final_readiness is None or bool(final_readiness["ready"]):
        stop_reason = "ready" if until_ready and final_readiness is not None else "single_pass"
        return True, stop_reason
    if coin_ids:
        return True, "requires_new_coin_selection"
    if iteration == max_iterations:
        return True, "max_iterations_reached"
    return False, ""


def _coin_op_unresolved_error(
    *,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    unresolved_coin_ids: list[str],
) -> str:
    return _format_json_output(
        {
            **_coin_op_base_payload(market, selected_venue, wallet),
            "waited": False,
            "success": False,
            "error": "coin_id_resolution_failed",
            "unknown_coin_ids": unresolved_coin_ids,
            "operator_guidance": (
                "run greenfloor-manager coins-list and pass coin_id values from output; "
                "manager accepts hex coin names and resolves them to Cloud Wallet Coin_* ids"
            ),
        }
    )


def _coin_op_result_payload(
    *,
    market: Any,
    selected_venue: str | None,
    wallet: CloudWalletAdapter,
    coin_ids: list[str],
    denomination_target: dict[str, Any] | None,
    until_ready: bool,
    max_iterations: int,
    stop_reason: str,
    final_readiness: dict[str, int | bool | str] | None,
    operations: list[dict[str, object]],
    fee_mojos: int,
    fee_source: str,
) -> dict[str, object]:
    return {
        **_coin_op_base_payload(market, selected_venue, wallet),
        "coin_selection_mode": "explicit" if coin_ids else "adapter_auto_select",
        "denomination_target": denomination_target,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "denomination_readiness": final_readiness,
        "operations": operations,
        "signature_request_id": (
            str(operations[-1].get("signature_request_id", "")) if operations else ""
        ),
        "signature_state": (
            str(operations[-1].get("signature_state", "UNKNOWN")) if operations else "UNKNOWN"
        ),
        "waited": bool(operations[-1].get("waited", False)) if operations else False,
        "wait_events": (
            _as_wait_events(operations[-1].get("wait_events", [])) if operations else []
        ),
        "fee_mojos": fee_mojos,
        "fee_source": fee_source,
    }


# ---------------------------------------------------------------------------
# Core commands
# ---------------------------------------------------------------------------


def _validate(program_path: Path, markets_path: Path) -> int:
    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)
    if program.python_min_version != "3.11":
        raise ValueError("program.yaml dev.python.min_version must be 3.11")
    for market in markets.markets:
        if market.enabled:
            resolve_market_key(
                market,
                signer_key_registry=program.signer_key_registry,
                required_network=program.app_network,
            )
    print("config validation ok")
    return 0


def _keys_onboard(
    *,
    program_path: Path,
    key_id: str,
    state_dir: Path,
    chia_keys_dir: Path | None = None,
) -> int:
    program = load_program_config(program_path)
    if not key_id.strip():
        raise ValueError("key_id must be provided")
    discovery = discover_chia_keys(chia_keys_dir)
    branch = determine_onboarding_branch(
        has_existing_keys=discovery.has_existing_keys,
        use_existing_keys=None,
        fallback_choice=None,
    )

    use_existing_keys = False
    if branch == "prompt_use_existing_keys":
        raw = (
            input(
                f"Found existing Chia keys at '{discovery.chia_keys_dir}'. Use these keys? [Y/n]: "
            )
            .strip()
            .lower()
        )
        use_existing_keys = raw in {"", "y", "yes"}
        branch = determine_onboarding_branch(
            has_existing_keys=discovery.has_existing_keys,
            use_existing_keys=use_existing_keys,
            fallback_choice=None,
        )

    if branch == "use_chia_keys":
        selection = KeyOnboardingSelection(
            selected_source="chia_keys",
            key_id=key_id,
            network=program.app_network,
            chia_keys_dir=str(discovery.chia_keys_dir),
            keyring_yaml_path=str(discovery.keyring_yaml_path),
        )
        selection_path = save_key_onboarding_selection(
            state_dir / "key_onboarding.json",
            selection,
        )
        print(
            _format_json_output(
                {
                    "selected_source": "chia_keys",
                    "key_id": key_id,
                    "network": program.app_network,
                    "chia_keys_dir": str(discovery.chia_keys_dir),
                    "keyring_yaml_path": str(discovery.keyring_yaml_path),
                    "selection_path": str(selection_path),
                    "next": "unlock_on_demand",
                }
            )
        )
        return 0

    raw_choice = input(
        "No Chia keyring selected. Choose key onboarding path: [1] add existing words, [2] generate new key: "
    ).strip()
    fallback_choice = (
        "import_words" if raw_choice == "1" else "generate_new" if raw_choice == "2" else ""
    )
    if fallback_choice == "":
        raise ValueError("invalid onboarding choice; expected 1 or 2")
    branch = determine_onboarding_branch(
        has_existing_keys=discovery.has_existing_keys,
        use_existing_keys=False,
        fallback_choice=fallback_choice,
    )

    if branch == "import_words":
        mnemonic = input("Enter existing mnemonic words: ").strip()
        words = [w for w in mnemonic.split() if w]
        if len(words) not in {12, 24}:
            raise ValueError("mnemonic must contain 12 or 24 words")
        selection = KeyOnboardingSelection(
            selected_source="mnemonic_import",
            key_id=key_id,
            network=program.app_network,
            mnemonic_word_count=len(words),
        )
        selection_path = save_key_onboarding_selection(
            state_dir / "key_onboarding.json",
            selection,
        )
        print(
            _format_json_output(
                {
                    "selected_source": "mnemonic_import",
                    "key_id": key_id,
                    "network": program.app_network,
                    "mnemonic_word_count": len(words),
                    "selection_path": str(selection_path),
                    "next": "store_in_secret_manager_then_set_key_id_mapping",
                }
            )
        )
        return 0

    selection = KeyOnboardingSelection(
        selected_source="generate_new_key",
        key_id=key_id,
        network=program.app_network,
    )
    selection_path = save_key_onboarding_selection(
        state_dir / "key_onboarding.json",
        selection,
    )
    print(
        _format_json_output(
            {
                "selected_source": "generate_new_key",
                "key_id": key_id,
                "network": program.app_network,
                "selection_path": str(selection_path),
                "next": "generate_and_store_with_wallet_sdk_key_provider",
            }
        )
    )
    return 0


def _resolve_dexie_base_url(network: str, explicit_base_url: str | None) -> str:
    if explicit_base_url and explicit_base_url.strip():
        return explicit_base_url.strip().rstrip("/")
    network_l = network.strip().lower()
    if network_l in {"mainnet", ""}:
        return "https://api.dexie.space"
    if network_l in {"testnet", "testnet11"}:
        return "https://api-testnet.dexie.space"
    raise ValueError(f"unsupported network for dexie posting: {network}")


def _resolve_splash_base_url(explicit_base_url: str | None) -> str:
    if explicit_base_url and explicit_base_url.strip():
        return explicit_base_url.strip().rstrip("/")
    return "http://john-deere.hoffmang.com:4000"


def _resolve_offer_publish_settings(
    *,
    program_path: Path,
    network: str,
    venue_override: str | None,
    dexie_base_url: str | None,
    splash_base_url: str | None,
) -> tuple[str, str, str]:
    program = load_program_config(program_path)
    venue = (venue_override or program.offer_publish_venue).strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("offer publish venue must be dexie or splash")
    if dexie_base_url and dexie_base_url.strip():
        dexie_base = dexie_base_url.strip().rstrip("/")
    elif network.strip().lower() in {"testnet", "testnet11"}:
        dexie_base = _resolve_dexie_base_url(network, None)
    else:
        dexie_base = str(program.dexie_api_base).strip().rstrip("/")
    if splash_base_url and splash_base_url.strip():
        splash_base = splash_base_url.strip().rstrip("/")
    else:
        splash_base = str(program.splash_api_base).strip().rstrip("/")
    return venue, dexie_base, splash_base


def _resolve_venue_for_coin_prep(*, venue_override: str | None) -> str | None:
    if venue_override is None or not venue_override.strip():
        return None
    venue = venue_override.strip().lower()
    if venue not in {"dexie", "splash"}:
        raise ValueError("coin-prep venue must be dexie or splash when provided")
    return venue


def _resolve_market_denomination_entry(market, *, size_base_units: int):
    ladder = market.ladders.get("sell") or []
    if not ladder:
        raise ValueError(
            f"market {market.market_id} has no sell ladder; cannot resolve denomination target"
        )
    for entry in ladder:
        if int(entry.size_base_units) == int(size_base_units):
            return entry
    allowed = ", ".join(str(int(row.size_base_units)) for row in ladder)
    raise ValueError(
        f"size_base_units not configured for market sell ladder; use one of: {allowed}"
    )


def _build_offer_text_for_request(payload: dict) -> str:
    return build_offer_text(payload)


def _build_and_post_offer_cloud_wallet(
    *,
    program,
    market,
    size_base_units: int,
    repeat: int,
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    quote_price: float,
    dry_run: bool,
) -> int:
    wallet = _new_cloud_wallet_adapter(program)
    resolved_base_asset_id, resolved_quote_asset_id = _resolve_cloud_wallet_offer_asset_ids(
        wallet=wallet,
        base_asset_id=str(market.base_asset),
        quote_asset_id=str(market.quote_asset),
        base_symbol_hint=str(getattr(market, "base_symbol", "") or ""),
        quote_symbol_hint=str(getattr(market, "quote_asset", "") or ""),
    )
    db_path = (Path(program.home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()
    store = SqliteStore(db_path)
    post_results: list[dict] = []
    built_offers_preview: list[dict[str, str]] = []
    publish_failures = 0
    offer_fee_mojos, offer_fee_source = _resolve_maker_offer_fee(network=program.app_network)
    dexie = DexieAdapter(dexie_base_url) if (not dry_run and publish_venue == "dexie") else None
    splash = SplashAdapter(splash_base_url) if (not dry_run and publish_venue == "splash") else None

    for _ in range(repeat):
        prior_wallet_payload = wallet.get_wallet()
        prior_offers = prior_wallet_payload.get("offers", [])
        known_offer_markers = _offer_markers(prior_offers if isinstance(prior_offers, list) else [])
        offer_amount = int(
            size_base_units * int((market.pricing or {}).get("base_unit_mojo_multiplier", 1000))
        )
        request_amount = int(
            round(
                float(size_base_units)
                * float(quote_price)
                * int((market.pricing or {}).get("quote_unit_mojo_multiplier", 1000))
            )
        )
        if request_amount <= 0:
            raise ValueError("request_amount must be positive")

        offered = [{"assetId": resolved_base_asset_id, "amount": offer_amount}]
        requested = [{"assetId": resolved_quote_asset_id, "amount": request_amount}]
        expires_at = (
            dt.datetime.now(dt.UTC) + dt.timedelta(minutes=_TEST_PHASE_OFFER_EXPIRY_MINUTES)
        ).isoformat()
        create_result = wallet.create_offer(
            offered=offered,
            requested=requested,
            fee=offer_fee_mojos,
            expires_at_iso=expires_at,
        )
        signature_request_id = str(create_result.get("signature_request_id", "")).strip()
        wait_events: list[dict[str, str]] = []
        signature_state = str(create_result.get("status", "UNKNOWN")).strip()
        if signature_request_id:
            signature_state, signature_wait_events = _poll_signature_request_until_not_unsigned(
                wallet=wallet,
                signature_request_id=signature_request_id,
                timeout_seconds=15 * 60,
                warning_interval_seconds=10 * 60,
            )
            wait_events.extend(signature_wait_events)

        offer_text = ""
        try:
            offer_text = _poll_offer_artifact_until_available(
                wallet=wallet,
                known_markers=known_offer_markers,
                timeout_seconds=15 * 60,
            )
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
                    },
                }
            )
            continue

        verify_error = _verify_offer_text_for_dexie(offer_text)
        if verify_error:
            publish_failures += 1
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {"success": False, "error": verify_error},
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

        if publish_venue == "dexie":
            assert dexie is not None
            result = dexie.post_offer(
                offer_text,
                drop_only=drop_only,
                claim_rewards=claim_rewards,
            )
        else:
            assert splash is not None
            result = splash.post_offer(offer_text)
        if result.get("success") is False:
            publish_failures += 1
        offer_id = str(result.get("id", "")).strip()
        if offer_id:
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=str(market.market_id),
                state=OfferLifecycleState.OPEN.value,
                last_seen_status=None,
            )
            store.add_audit_event(
                "strategy_offer_execution",
                {
                    "offer_id": offer_id,
                    "market_id": str(market.market_id),
                    "venue": publish_venue,
                    "signature_request_id": signature_request_id,
                    "signature_state": signature_state,
                    "resolved_base_asset_id": resolved_base_asset_id,
                    "resolved_quote_asset_id": resolved_quote_asset_id,
                },
                market_id=str(market.market_id),
            )
        post_results.append(
            {
                "venue": publish_venue,
                "result": {
                    **result,
                    "signature_request_id": signature_request_id,
                    "signature_state": signature_state,
                    "wait_events": wait_events,
                },
            }
        )

    print(
        _format_json_output(
            {
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
                "results": post_results,
                "offer_fee_mojos": offer_fee_mojos,
                "offer_fee_source": offer_fee_source,
            }
        )
    )
    store.close()
    return 0 if publish_failures == 0 else 2


def _build_and_post_offer(
    *,
    program_path: Path,
    markets_path: Path,
    network: str,
    market_id: str | None,
    pair: str | None,
    size_base_units: int,
    repeat: int,
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
) -> int:
    if size_base_units <= 0:
        raise ValueError("size_base_units must be positive")
    if repeat <= 0:
        raise ValueError("repeat must be positive")

    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)
    market = _resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    signer_key = program.signer_key_registry.get(market.signer_key_id)
    keyring_yaml_path = signer_key.keyring_yaml_path if signer_key is not None else ""
    pricing = dict(getattr(market, "pricing", {}) or {})
    quote_price = pricing.get("fixed_quote_per_base")
    if quote_price is None:
        min_q = pricing.get("min_price_quote_per_base")
        max_q = pricing.get("max_price_quote_per_base")
        if min_q is not None and max_q is not None:
            quote_price = (float(min_q) + float(max_q)) / 2.0
        elif min_q is not None:
            quote_price = float(min_q)
        elif max_q is not None:
            quote_price = float(max_q)
    if quote_price is None:
        raise ValueError(
            "market pricing must define fixed_quote_per_base or min/max_price_quote_per_base for offer build"
        )

    cloud_wallet_configured = (
        bool(program.cloud_wallet_base_url)
        and bool(program.cloud_wallet_user_key_id)
        and bool(program.cloud_wallet_private_key_pem_path)
        and bool(program.cloud_wallet_vault_id)
    )
    if cloud_wallet_configured:
        return _build_and_post_offer_cloud_wallet(
            program=program,
            market=market,
            size_base_units=size_base_units,
            repeat=repeat,
            publish_venue=publish_venue,
            dexie_base_url=dexie_base_url,
            splash_base_url=splash_base_url,
            drop_only=drop_only,
            claim_rewards=claim_rewards,
            quote_price=float(quote_price),
            dry_run=bool(dry_run),
        )

    debug_dry_run_offer_capture_dir = os.getenv(
        "GREENFLOOR_DEBUG_DRY_RUN_OFFER_CAPTURE_DIR", ""
    ).strip()
    capture_dir_path = (
        Path(debug_dry_run_offer_capture_dir).expanduser()
        if debug_dry_run_offer_capture_dir
        else None
    )
    if dry_run and capture_dir_path is not None:
        capture_dir_path.mkdir(parents=True, exist_ok=True)

    post_results: list[dict] = []
    built_offers_preview: list[dict[str, str]] = []
    dexie = DexieAdapter(dexie_base_url) if (not dry_run and publish_venue == "dexie") else None
    splash = SplashAdapter(splash_base_url) if (not dry_run and publish_venue == "splash") else None
    publish_failures = 0
    for index in range(repeat):
        payload = {
            "market_id": market.market_id,
            "base_asset": market.base_asset,
            "base_symbol": market.base_symbol,
            "quote_asset": market.quote_asset,
            "quote_asset_type": market.quote_asset_type,
            "receive_address": market.receive_address,
            "size_base_units": int(size_base_units),
            "pair": str(market.quote_asset).strip().lower(),
            "reason": "manual_build_and_post",
            "xch_price_usd": None,
            "expiry_unit": "minutes",
            "expiry_value": _TEST_PHASE_OFFER_EXPIRY_MINUTES,
            "quote_price_quote_per_base": float(quote_price),
            "base_unit_mojo_multiplier": int(pricing.get("base_unit_mojo_multiplier", 1000)),
            "quote_unit_mojo_multiplier": int(pricing.get("quote_unit_mojo_multiplier", 1000)),
            "fee_mojos": 0,
            "dry_run": bool(dry_run),
            "key_id": market.signer_key_id,
            "keyring_yaml_path": keyring_yaml_path,
            "network": network,
            "asset_id": market.base_asset,
        }
        try:
            offer_text = _build_offer_text_for_request(payload)
        except Exception as exc:
            publish_failures += 1
            post_results.append(
                {
                    "venue": publish_venue,
                    "result": {
                        "success": False,
                        "error": f"offer_builder_failed:{exc}",
                    },
                }
            )
            continue
        if dry_run:
            preview_item: dict[str, str] = {
                "offer_prefix": offer_text[:24],
                "offer_length": str(len(offer_text)),
            }
            if capture_dir_path is not None:
                capture_file = capture_dir_path / f"{market.market_id}-dry-run-{index + 1}.offer"
                capture_file.write_text(offer_text, encoding="utf-8")
                preview_item["offer_capture_path"] = str(capture_file)
            built_offers_preview.append(preview_item)
        else:
            if publish_venue == "dexie":
                assert dexie is not None
                verify_error = _verify_offer_text_for_dexie(offer_text)
                if verify_error:
                    publish_failures += 1
                    post_results.append(
                        {
                            "venue": "dexie",
                            "result": {"success": False, "error": verify_error},
                        }
                    )
                    continue
                result = dexie.post_offer(
                    offer_text,
                    drop_only=drop_only,
                    claim_rewards=claim_rewards,
                )
                success_value = result.get("success")
                if success_value is False:
                    publish_failures += 1
                post_results.append({"venue": "dexie", "result": result})
            else:
                assert splash is not None
                result = splash.post_offer(offer_text)
                success_value = result.get("success")
                if success_value is False:
                    publish_failures += 1
                post_results.append({"venue": "splash", "result": result})

    publish_attempts = len(post_results)
    print(
        _format_json_output(
            {
                "market_id": market.market_id,
                "pair": f"{market.base_asset}:{market.quote_asset}",
                "network": network,
                "size_base_units": size_base_units,
                "repeat": repeat,
                "publish_venue": publish_venue,
                "dexie_base_url": dexie_base_url,
                "splash_base_url": splash_base_url if publish_venue == "splash" else None,
                "drop_only": drop_only,
                "claim_rewards": claim_rewards,
                "dry_run": dry_run,
                "publish_attempts": publish_attempts,
                "publish_failures": publish_failures,
                "built_offers_preview": built_offers_preview,
                "results": post_results,
            }
        )
    )
    return 0 if publish_failures == 0 else 2


def _resolve_market_for_build(
    markets,
    *,
    market_id: str | None,
    pair: str | None,
    network: str,
):
    if bool(market_id) == bool(pair):
        raise ValueError("provide exactly one of --market-id or --pair")
    if market_id:
        selected = next((m for m in markets.markets if m.market_id == market_id), None)
        if selected is None:
            raise ValueError(f"market_id not found: {market_id}")
        return selected

    assert pair is not None
    raw = pair.strip()
    sep = ":" if ":" in raw else "/" if "/" in raw else ""
    if not sep:
        raise ValueError("pair must be in base:quote or base/quote format")
    base_raw, quote_raw = [p.strip().lower() for p in raw.split(sep, 1)]
    if not base_raw or not quote_raw:
        raise ValueError("pair base and quote must be non-empty")
    network_l = network.strip().lower()
    candidates = []
    for market in markets.markets:
        if not market.enabled:
            continue
        base_matches = {
            str(market.base_asset).strip().lower(),
            str(market.base_symbol).strip().lower(),
        }
        quote_match = str(market.quote_asset).strip().lower()
        quote_matches = {quote_match}
        if network_l in {"testnet", "testnet11"}:
            if quote_match == "xch":
                quote_matches.add("txch")
            elif quote_match == "txch":
                quote_matches.add("xch")
        if base_raw in base_matches and quote_raw in quote_matches:
            candidates.append(market)
    if not candidates:
        raise ValueError(f"no enabled market found for pair: {pair}")
    if len(candidates) > 1:
        ids = ", ".join(sorted(m.market_id for m in candidates))
        raise ValueError(f"pair is ambiguous; use --market-id (candidates: {ids})")
    return candidates[0]


def _coins_list(
    *,
    program_path: Path,
    asset: str | None,
    vault_id: str | None,
) -> int:
    program = load_program_config(program_path)
    wallet = _new_cloud_wallet_adapter(program)
    if vault_id and vault_id.strip() and vault_id.strip() != wallet.vault_id:
        raise ValueError(
            "vault_id override is not supported with current cloud_wallet config; update program cloud_wallet.vault_id"
        )
    requested_asset = asset.strip() if asset else ""
    resolved_asset_filter: str | None = None
    if requested_asset:
        resolved_asset_filter = _resolve_cloud_wallet_asset_id(
            wallet=wallet,
            canonical_asset_id=requested_asset,
            symbol_hint=requested_asset,
        )
    coins = wallet.list_coins(asset_id=resolved_asset_filter, include_pending=True)
    items = []
    for coin in coins:
        coin_state = str(coin.get("state", "")).strip().upper()
        pending = coin_state in {"PENDING", "MEMPOOL"}
        spendable = _is_spendable_coin(coin)
        asset_raw = coin.get("asset")
        asset_id = "xch"
        if isinstance(asset_raw, dict):
            asset_id = str(asset_raw.get("id", "xch")).strip()
        items.append(
            {
                "coin_id": str(coin.get("name", coin.get("id", ""))).strip(),
                "amount": int(coin.get("amount", 0)),
                "state": coin_state or "UNKNOWN",
                "pending": pending,
                "spendable": spendable,
                "asset": asset_id,
            }
        )
    print(
        _format_json_output(
            {
                "vault_id": wallet.vault_id,
                "network": wallet.network,
                "count": len(items),
                "items": items,
            }
        )
    )
    return 0


def _coin_split(
    *,
    program_path: Path,
    markets_path: Path,
    network: str,
    market_id: str | None,
    pair: str | None,
    coin_ids: list[str],
    amount_per_coin: int,
    number_of_coins: int,
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
) -> int:
    program = load_program_config(program_path)
    selected_venue = _resolve_venue_for_coin_prep(venue_override=venue)
    markets = load_markets_config(markets_path)
    market = _resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    denomination_target = None
    if size_base_units is not None and int(size_base_units) > 0:
        entry = _resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        required_count = int(entry.target_count) + int(entry.split_buffer_count)
        if amount_per_coin <= 0:
            amount_per_coin = int(entry.size_base_units)
        elif amount_per_coin != int(entry.size_base_units):
            raise ValueError(
                "amount_per_coin must match market ladder size when --size-base-units is set"
            )
        if number_of_coins <= 0:
            number_of_coins = required_count
        elif number_of_coins != required_count:
            raise ValueError(
                "number_of_coins must match market ladder target+buffer when --size-base-units is set"
            )
        denomination_target = {
            "size_base_units": int(entry.size_base_units),
            "target_count": int(entry.target_count),
            "split_buffer_count": int(entry.split_buffer_count),
            "required_count": required_count,
        }
    if amount_per_coin <= 0:
        raise ValueError("amount_per_coin must be positive")
    if number_of_coins <= 0:
        raise ValueError("number_of_coins must be positive")
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and denomination_target is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")
    wallet = _new_cloud_wallet_adapter(program)
    resolved_split_asset_id = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=str(market.base_asset),
        symbol_hint=str(market.base_symbol),
    )
    fee_result = _resolve_coin_op_fee(
        network=network,
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        market=market,
        selected_venue=selected_venue,
        wallet=wallet,
    )
    if fee_result is None:
        return 2
    fee_mojos, fee_source = fee_result

    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        wallet_coins = wallet.list_coins(include_pending=True)
        existing_coin_ids = {str(c.get("id", "")).strip() for c in wallet_coins}
        if coin_ids:
            resolved_coin_ids, unresolved_coin_ids = _resolve_coin_global_ids(
                wallet_coins, coin_ids
            )
            if unresolved_coin_ids:
                break
        else:
            asset_scoped_coins = wallet.list_coins(
                asset_id=resolved_split_asset_id,
                include_pending=True,
            )
            spendable_asset_coins = [c for c in asset_scoped_coins if _is_spendable_coin(c)]
            if not spendable_asset_coins:
                print(
                    _format_json_output(
                        {
                            **_coin_op_base_payload(market, selected_venue, wallet),
                            "waited": False,
                            "success": False,
                            "error": "no_spendable_split_coin_available",
                            "asset_id": str(market.base_asset),
                            "resolved_asset_id": resolved_split_asset_id,
                            "operator_guidance": (
                                "no spendable coins are currently available for this asset; "
                                "wait for pending/signature requests to settle or free locked offers, "
                                "then retry coin-split"
                            ),
                        }
                    )
                )
                return 2
            selected_coin = max(
                spendable_asset_coins,
                key=lambda coin: int(coin.get("amount", 0)),
            )
            selected_coin_global_id = str(selected_coin.get("id", "")).strip()
            if not selected_coin_global_id:
                raise RuntimeError("coin_split_failed:missing_selected_coin_id")
            resolved_coin_ids = [selected_coin_global_id]
            unresolved_coin_ids = []

        split_result = wallet.split_coins(
            coin_ids=resolved_coin_ids,
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=fee_mojos,
        )
        signature_request_id = split_result["signature_request_id"]
        if not signature_request_id:
            raise RuntimeError("coin_split_failed:missing_signature_request_id")

        readiness_kwargs: dict[str, int] = {}
        if denomination_target is not None:
            readiness_kwargs["required_min_count"] = int(denomination_target["required_count"])
        iteration_payload, final_readiness = _coin_op_build_iteration_payload(
            wallet=wallet,
            signature_request_id=signature_request_id,
            initial_signature_state=split_result.get("status", "UNKNOWN"),
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            denomination_target=denomination_target,
            readiness_asset_id=str(market.base_asset),
            readiness_kwargs=readiness_kwargs,
        )
        operations.append(iteration_payload)

        should_break, reason = _coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    if unresolved_coin_ids:
        print(
            _coin_op_unresolved_error(
                market=market,
                selected_venue=selected_venue,
                wallet=wallet,
                unresolved_coin_ids=unresolved_coin_ids,
            )
        )
        return 2
    print(
        _format_json_output(
            {
                **_coin_op_result_payload(
                    market=market,
                    selected_venue=selected_venue,
                    wallet=wallet,
                    coin_ids=coin_ids,
                    denomination_target=denomination_target,
                    until_ready=until_ready,
                    max_iterations=max_iterations,
                    stop_reason=stop_reason,
                    final_readiness=final_readiness,
                    operations=operations,
                    fee_mojos=fee_mojos,
                    fee_source=fee_source,
                ),
                "amount_per_coin": amount_per_coin,
                "number_of_coins": number_of_coins,
                "resolved_asset_id": resolved_split_asset_id,
            }
        )
    )
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0


def _coin_combine(
    *,
    program_path: Path,
    markets_path: Path,
    network: str,
    market_id: str | None,
    pair: str | None,
    number_of_coins: int,
    asset_id: str | None,
    coin_ids: list[str],
    no_wait: bool,
    venue: str | None = None,
    size_base_units: int | None = None,
    until_ready: bool = False,
    max_iterations: int = 3,
) -> int:
    program = load_program_config(program_path)
    selected_venue = _resolve_venue_for_coin_prep(venue_override=venue)
    markets = load_markets_config(markets_path)
    market = _resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    denomination_target = None
    requested_asset_id = asset_id.strip() if asset_id else str(market.base_asset).strip()
    if size_base_units is not None and int(size_base_units) > 0:
        entry = _resolve_market_denomination_entry(market, size_base_units=int(size_base_units))
        threshold = max(
            2,
            int(math.ceil(int(entry.target_count) * float(entry.combine_when_excess_factor))),
        )
        if number_of_coins <= 0:
            number_of_coins = threshold
        elif number_of_coins != threshold:
            raise ValueError(
                "number_of_coins must match market ladder combine threshold when --size-base-units is set"
            )
        denomination_target = {
            "size_base_units": int(entry.size_base_units),
            "target_count": int(entry.target_count),
            "combine_when_excess_factor": float(entry.combine_when_excess_factor),
            "combine_threshold_count": threshold,
        }
    if number_of_coins <= 1:
        raise ValueError("number_of_coins must be > 1")
    if until_ready and no_wait:
        raise ValueError("until-ready mode requires wait mode (do not pass --no-wait)")
    if until_ready and denomination_target is None:
        raise ValueError("until-ready mode requires --size-base-units")
    if max_iterations <= 0:
        raise ValueError("max_iterations must be positive")
    wallet = _new_cloud_wallet_adapter(program)
    resolved_asset_id = _resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=requested_asset_id,
        symbol_hint=str(market.base_symbol).strip() if not asset_id else requested_asset_id,
    )
    fee_result = _resolve_coin_op_fee(
        network=network,
        minimum_fee_mojos=int(program.coin_ops_minimum_fee_mojos),
        market=market,
        selected_venue=selected_venue,
        wallet=wallet,
    )
    if fee_result is None:
        return 2
    fee_mojos, fee_source = fee_result

    operations: list[dict[str, object]] = []
    final_readiness: dict[str, int | bool | str] | None = None
    stop_reason = "single_pass"
    unresolved_coin_ids: list[str] = []

    for iteration in range(1, max_iterations + 1):
        wallet_coins = wallet.list_coins(include_pending=True)
        existing_coin_ids = {str(c.get("id", "")).strip() for c in wallet_coins}
        resolved_input_coin_ids: list[str] | None = None
        if coin_ids:
            resolved_input_coin_ids, unresolved_coin_ids = _resolve_coin_global_ids(
                wallet_coins, coin_ids
            )
            if unresolved_coin_ids:
                break
            if number_of_coins != len(resolved_input_coin_ids):
                raise ValueError(
                    "when --coin-id is provided, --input-coin-count must match the number of --coin-id values"
                )

        combine_result = wallet.combine_coins(
            number_of_coins=number_of_coins,
            fee=fee_mojos,
            asset_id=resolved_asset_id,
            largest_first=True,
            input_coin_ids=resolved_input_coin_ids,
        )
        signature_request_id = combine_result["signature_request_id"]
        if not signature_request_id:
            raise RuntimeError("coin_combine_failed:missing_signature_request_id")

        readiness_kwargs: dict[str, int] = {}
        if denomination_target is not None:
            readiness_kwargs["max_allowed_count"] = int(
                denomination_target["combine_threshold_count"]
            )
        iteration_payload, final_readiness = _coin_op_build_iteration_payload(
            wallet=wallet,
            signature_request_id=signature_request_id,
            initial_signature_state=combine_result.get("status", "UNKNOWN"),
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            denomination_target=denomination_target,
            readiness_asset_id=resolved_asset_id,
            readiness_kwargs=readiness_kwargs,
        )
        operations.append(iteration_payload)

        should_break, reason = _coin_op_should_stop(
            until_ready=until_ready,
            final_readiness=final_readiness,
            coin_ids=coin_ids,
            iteration=iteration,
            max_iterations=max_iterations,
        )
        if should_break:
            stop_reason = reason
            break

    if unresolved_coin_ids:
        print(
            _coin_op_unresolved_error(
                market=market,
                selected_venue=selected_venue,
                wallet=wallet,
                unresolved_coin_ids=unresolved_coin_ids,
            )
        )
        return 2
    print(
        _format_json_output(
            {
                **_coin_op_result_payload(
                    market=market,
                    selected_venue=selected_venue,
                    wallet=wallet,
                    coin_ids=coin_ids,
                    denomination_target=denomination_target,
                    until_ready=until_ready,
                    max_iterations=max_iterations,
                    stop_reason=stop_reason,
                    final_readiness=final_readiness,
                    operations=operations,
                    fee_mojos=fee_mojos,
                    fee_source=fee_source,
                ),
                "asset_id": requested_asset_id,
                "resolved_asset_id": resolved_asset_id,
                "number_of_coins": number_of_coins,
            }
        )
    )
    if until_ready and final_readiness is not None and not bool(final_readiness["ready"]):
        return 2
    return 0


def _resolve_db_path(program_config_path: Path, explicit_db_path: str | None) -> Path:
    if explicit_db_path:
        return Path(explicit_db_path).expanduser()
    program = load_program_config(program_config_path)
    return (Path(program.home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()


def _bootstrap_home(
    *,
    home_dir: Path,
    program_template: Path,
    markets_template: Path,
    force: bool,
) -> int:
    home = home_dir.expanduser().resolve()
    config_dir = home / "config"
    db_dir = home / "db"
    state_dir = home / "state"
    logs_dir = home / "logs"

    for p in (home, config_dir, db_dir, state_dir, logs_dir):
        p.mkdir(parents=True, exist_ok=True)

    seeded_program = config_dir / "program.yaml"
    seeded_markets = config_dir / "markets.yaml"

    wrote_program = False
    if force or not seeded_program.exists():
        program_data = load_yaml(program_template)
        app = dict(program_data.get("app", {}))
        app["home_dir"] = str(home)
        program_data["app"] = app
        seeded_program.write_text(
            yaml.safe_dump(program_data, sort_keys=False),
            encoding="utf-8",
        )
        wrote_program = True

    wrote_markets = False
    if force or not seeded_markets.exists():
        markets_data = load_yaml(markets_template)
        seeded_markets.write_text(
            yaml.safe_dump(markets_data, sort_keys=False),
            encoding="utf-8",
        )
        wrote_markets = True

    db_path = (db_dir / "greenfloor.sqlite").resolve()
    store = SqliteStore(db_path)
    try:
        store.add_audit_event(
            "home_bootstrap",
            {
                "home_dir": str(home),
                "program_config": str(seeded_program),
                "markets_config": str(seeded_markets),
                "force": bool(force),
            },
        )
    finally:
        store.close()

    print(
        _format_json_output(
            {
                "bootstrapped": True,
                "home_dir": str(home),
                "program_config": str(seeded_program),
                "markets_config": str(seeded_markets),
                "state_db": str(db_path),
                "state_dir": str(state_dir),
                "logs_dir": str(logs_dir),
                "wrote_program_config": wrote_program,
                "wrote_markets_config": wrote_markets,
            }
        )
    )
    return 0


def _doctor(program_path: Path, markets_path: Path, state_db: str | None) -> int:
    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)

    problems: list[str] = []
    warnings: list[str] = []

    enabled_markets = [m for m in markets.markets if m.enabled]
    if not enabled_markets:
        warnings.append("no_enabled_markets")

    key_ids = []
    for market in enabled_markets:
        try:
            resolved = resolve_market_key(
                market,
                signer_key_registry=program.signer_key_registry,
                required_network=program.app_network,
            )
            key_ids.append(resolved.key_id)
        except Exception as exc:
            problems.append(f"market_key_error:{market.market_id}:{exc}")

    db_path = _resolve_db_path(program_path, state_db)
    try:
        store = SqliteStore(db_path)
        store.add_audit_event("doctor_ping", {"ok": True})
        store.close()
    except Exception as exc:
        problems.append(f"db_error:{exc}")

    if program.tx_block_webhook_enabled and ":" not in program.tx_block_webhook_listen_addr:
        problems.append("invalid_webhook_listen_addr")

    if program.pushover_enabled:
        if not os.getenv(program.pushover_app_token_env):
            warnings.append(f"missing_env:{program.pushover_app_token_env}")
        if not (
            os.getenv(program.pushover_user_key_env)
            or os.getenv(program.pushover_recipient_key_env)
        ):
            warnings.append(
                f"missing_env:{program.pushover_user_key_env}|{program.pushover_recipient_key_env}"
            )

    def _warn_if_invalid_int_env(name: str, minimum: int = 0) -> None:
        raw = os.getenv(name, "").strip()
        if not raw:
            return
        try:
            value = int(raw)
        except ValueError:
            warnings.append(f"invalid_env_override:{name}:not_integer")
            return
        if value < minimum:
            warnings.append(f"invalid_env_override:{name}:must_be>={minimum}")

    _warn_if_invalid_int_env("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", minimum=1)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", minimum=1)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_POST_BACKOFF_MS", minimum=0)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", minimum=0)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", minimum=1)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", minimum=0)
    _warn_if_invalid_int_env("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", minimum=0)

    if not program.cloud_wallet_base_url:
        warnings.append("cloud_wallet_not_configured:base_url")
    if not program.cloud_wallet_user_key_id:
        warnings.append("cloud_wallet_not_configured:user_key_id")
    if not program.cloud_wallet_private_key_pem_path:
        warnings.append("cloud_wallet_not_configured:private_key_pem_path")
    if not program.cloud_wallet_vault_id:
        warnings.append("cloud_wallet_not_configured:vault_id")

    result = {
        "ok": len(problems) == 0,
        "program_config": str(program_path),
        "markets_config": str(markets_path),
        "state_db": str(db_path),
        "enabled_markets": len(enabled_markets),
        "resolved_key_ids": sorted(set(key_ids)),
        "warnings": warnings,
        "problems": problems,
    }
    print(_format_json_output(result))
    return 0 if not problems else 2


def _reconciled_state_from_dexie_status(
    *,
    status: int,
    current_state: str,
) -> str:
    if status == 4:
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.TX_CONFIRMED,
        )
        return transition.new_state.value
    if status == 6:
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.EXPIRED,
        )
        return transition.new_state.value
    if status == 3:
        return "cancelled"
    if status in {0, 1, 2, 5}:
        if current_state in {
            OfferLifecycleState.TX_BLOCK_CONFIRMED.value,
            OfferLifecycleState.EXPIRED.value,
            "cancelled",
        }:
            return current_state
        transition = apply_offer_signal(
            OfferLifecycleState.OPEN,
            OfferSignal.MEMPOOL_SEEN,
        )
        return transition.new_state.value
    return "unknown_orphaned"


def _offers_reconcile(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    venue: str | None,
) -> int:
    db_path = _resolve_db_path(program_path, state_db)
    store = SqliteStore(db_path)
    try:
        program = load_program_config(program_path)
        target_venue = str(venue or program.offer_publish_venue).strip().lower()
        rows = store.list_offer_states(market_id=market_id, limit=limit)
        items: list[dict] = []
        reconciled = 0
        changed = 0
        for row in rows:
            offer_id = str(row["offer_id"])
            market_value = str(row["market_id"])
            current_state = str(row["state"])
            taker_signal = "none"
            taker_diagnostic = "none"
            signal_source = "none"
            coinset_tx_ids: list[str] = []
            coinset_confirmed_tx_ids: list[str] = []
            coinset_mempool_tx_ids: list[str] = []
            if target_venue != "dexie":
                next_state = "reconcile_unsupported_venue"
                reason = f"unsupported_venue:{target_venue}"
                status = None
                changed_flag = next_state != current_state
            else:
                adapter = DexieAdapter(program.dexie_api_base)
                status: int | None
                reason = "ok"
                try:
                    payload = adapter.get_offer(offer_id)
                    raw_status = payload.get("status")
                    status = int(raw_status) if raw_status is not None else None
                    coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(payload)
                    if coinset_tx_ids:
                        signal_by_tx_id = store.get_tx_signal_state(coinset_tx_ids)
                        for tx_id in coinset_tx_ids:
                            signal = signal_by_tx_id.get(tx_id, {})
                            if signal.get("tx_block_confirmed_at"):
                                coinset_confirmed_tx_ids.append(tx_id)
                                continue
                            if signal.get("mempool_observed_at"):
                                coinset_mempool_tx_ids.append(tx_id)
                    if coinset_confirmed_tx_ids and status != 3 and current_state != "cancelled":
                        transition = apply_offer_signal(
                            OfferLifecycleState.OPEN,
                            OfferSignal.TX_CONFIRMED,
                        )
                        next_state = transition.new_state.value
                        reason = "coinset_tx_block_webhook_confirmed"
                        signal_source = "coinset_webhook"
                    elif coinset_mempool_tx_ids:
                        if current_state in {
                            OfferLifecycleState.TX_BLOCK_CONFIRMED.value,
                            OfferLifecycleState.EXPIRED.value,
                            "cancelled",
                        }:
                            next_state = current_state
                        else:
                            transition = apply_offer_signal(
                                OfferLifecycleState.OPEN,
                                OfferSignal.MEMPOOL_SEEN,
                            )
                            next_state = transition.new_state.value
                        reason = "coinset_mempool_observed"
                        signal_source = "coinset_mempool"
                    if status is None:
                        if not coinset_tx_ids:
                            next_state = "unknown_orphaned"
                            reason = "missing_status"
                        elif signal_source == "none":
                            next_state = current_state
                            reason = "coinset_signal_unavailable_for_offer"
                    else:
                        if signal_source == "none":
                            next_state = _reconciled_state_from_dexie_status(
                                status=status,
                                current_state=current_state,
                            )
                            signal_source = "dexie_status_fallback"
                except urllib.error.HTTPError as exc:
                    status = None
                    if int(getattr(exc, "code", 0)) == 404:
                        next_state = "unknown_orphaned"
                        reason = "dexie_offer_not_found"
                    else:
                        next_state = current_state
                        reason = f"dexie_http_error:{exc.code}"
                except Exception as exc:
                    status = None
                    next_state = current_state
                    reason = f"dexie_lookup_error:{exc}"
                changed_flag = next_state != current_state
            if (
                coinset_confirmed_tx_ids
                and status != 3
                and current_state != "cancelled"
                and next_state == OfferLifecycleState.TX_BLOCK_CONFIRMED.value
            ):
                taker_signal = "coinset_tx_block_webhook"
                taker_diagnostic = "coinset_tx_block_confirmed"
            elif coinset_mempool_tx_ids:
                taker_diagnostic = "coinset_mempool_observed"
            elif status in {4, 5}:
                taker_diagnostic = "dexie_status_pattern_fallback"
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=market_value,
                state=next_state,
                last_seen_status=status,
            )
            store.add_audit_event(
                "offer_reconciliation",
                {
                    "offer_id": offer_id,
                    "market_id": market_value,
                    "venue": target_venue,
                    "old_state": current_state,
                    "new_state": next_state,
                    "changed": changed_flag,
                    "last_seen_status": status,
                    "reason": reason,
                    "taker_signal": taker_signal,
                    "taker_diagnostic": taker_diagnostic,
                    "signal_source": signal_source,
                    "coinset_tx_ids": coinset_tx_ids,
                    "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    "coinset_mempool_tx_ids": coinset_mempool_tx_ids,
                },
                market_id=market_value,
            )
            if taker_signal != "none":
                store.add_audit_event(
                    "taker_detection",
                    {
                        "offer_id": offer_id,
                        "market_id": market_value,
                        "venue": target_venue,
                        "signal": taker_signal,
                        "advisory_diagnostic": taker_diagnostic,
                        "old_state": current_state,
                        "new_state": next_state,
                        "last_seen_status": status,
                        "signal_source": signal_source,
                        "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    },
                    market_id=market_value,
                )
            reconciled += 1
            changed += int(changed_flag)
            items.append(
                {
                    "offer_id": offer_id,
                    "market_id": market_value,
                    "old_state": current_state,
                    "new_state": next_state,
                    "changed": changed_flag,
                    "last_seen_status": status,
                    "reason": reason,
                    "taker_signal": taker_signal,
                    "taker_diagnostic": taker_diagnostic,
                    "signal_source": signal_source,
                    "coinset_tx_ids": coinset_tx_ids,
                    "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                    "coinset_mempool_tx_ids": coinset_mempool_tx_ids,
                }
            )
        print(
            _format_json_output(
                {
                    "state_db": str(db_path),
                    "venue": target_venue,
                    "market_id": market_id,
                    "reconciled_count": reconciled,
                    "changed_count": changed,
                    "items": items,
                }
            )
        )
    finally:
        store.close()
    return 0


def _offers_status(
    *,
    program_path: Path,
    state_db: str | None,
    market_id: str | None,
    limit: int,
    events_limit: int,
) -> int:
    db_path = _resolve_db_path(program_path, state_db)
    store = SqliteStore(db_path)
    try:
        offers = store.list_offer_states(market_id=market_id, limit=limit)
        events = store.list_recent_audit_events(
            event_types=[
                "strategy_offer_execution",
                "offer_cancel_policy",
                "offer_lifecycle_transition",
                "offer_reconciliation",
                "taker_detection",
                "dexie_offers_error",
            ],
            market_id=market_id,
            limit=events_limit,
        )
    finally:
        store.close()
    by_state: dict[str, int] = {}
    for row in offers:
        by_state[row["state"]] = by_state.get(row["state"], 0) + 1
    print(
        _format_json_output(
            {
                "state_db": str(db_path),
                "market_id": market_id,
                "offer_count": len(offers),
                "by_state": by_state,
                "offers": offers,
                "recent_events": events,
            }
        )
    )
    return 0


def _cloud_wallet_offer_ui_url(
    *, cloud_wallet_base_url: str, vault_id: str, wallet_offer_id: str
) -> str:
    raw = str(cloud_wallet_base_url).strip()
    if not raw:
        return ""
    parsed = urllib.parse.urlparse(raw)
    if not parsed.scheme or not parsed.netloc:
        return ""
    host = parsed.netloc
    if host.startswith("api."):
        host = host[4:]
    base = f"{parsed.scheme}://{host}"
    clean_vault = str(vault_id).strip()
    clean_offer = str(wallet_offer_id).strip()
    if not clean_vault or not clean_offer:
        return ""
    return f"{base}/wallet/{clean_vault}/offers/{clean_offer}"


def _offers_cancel(
    *,
    program_path: Path,
    offer_ids: list[str],
    cancel_open: bool,
) -> int:
    program = load_program_config(program_path)
    wallet = _new_cloud_wallet_adapter(program)
    requested_ids = [str(value).strip() for value in offer_ids if str(value).strip()]
    selected_offers: list[dict[str, str]] = []
    wallet_payload = wallet.get_wallet()
    offers = wallet_payload.get("offers", [])
    for row in offers if isinstance(offers, list) else []:
        if not isinstance(row, dict):
            continue
        selected_offers.append(
            {
                "wallet_offer_id": str(row.get("id", "")).strip(),
                "offer_id": str(row.get("offerId", "")).strip(),
                "state": str(row.get("state", "")).strip(),
                "expires_at": str(row.get("expiresAt", "")).strip(),
            }
        )
    selected_offers = [row for row in selected_offers if row["offer_id"]]
    if cancel_open:
        selected_offers = [
            row for row in selected_offers if str(row.get("state", "")).upper() == "OPEN"
        ]
    elif requested_ids:
        requested_set = set(requested_ids)
        selected_offers = [row for row in selected_offers if row["offer_id"] in requested_set]
    else:
        raise ValueError("provide at least one --offer-id or pass --cancel-open")

    items: list[dict[str, Any]] = []
    failures = 0
    for row in selected_offers:
        offer_id = row["offer_id"]
        wallet_offer_id = row.get("wallet_offer_id", "")
        ui_url = _cloud_wallet_offer_ui_url(
            cloud_wallet_base_url=str(program.cloud_wallet_base_url),
            vault_id=wallet.vault_id,
            wallet_offer_id=wallet_offer_id,
        )
        try:
            cancel_result = wallet.cancel_offer(offer_id=offer_id)
            item = {
                "offer_id": offer_id,
                "wallet_offer_id": wallet_offer_id,
                "state": row.get("state", ""),
                "expires_at": row.get("expires_at", ""),
                "url": ui_url,
                "result": {
                    "success": True,
                    "signature_request_id": str(
                        cancel_result.get("signature_request_id", "")
                    ).strip(),
                    "signature_state": str(cancel_result.get("status", "")).strip(),
                },
            }
            if not item["result"]["signature_request_id"]:
                failures += 1
                item["result"]["success"] = False
                item["result"]["error"] = "cancel_offer_missing_signature_request_id"
            items.append(item)
        except Exception as exc:
            failures += 1
            items.append(
                {
                    "offer_id": offer_id,
                    "wallet_offer_id": wallet_offer_id,
                    "state": row.get("state", ""),
                    "expires_at": row.get("expires_at", ""),
                    "url": ui_url,
                    "result": {
                        "success": False,
                        "error": str(exc),
                    },
                }
            )
    print(
        _format_json_output(
            {
                "vault_id": wallet.vault_id,
                "cancel_open": bool(cancel_open),
                "requested_offer_ids": requested_ids,
                "selected_count": len(selected_offers),
                "cancelled_count": len(selected_offers) - failures,
                "failed_count": failures,
                "items": items,
            }
        )
    )
    return 0 if failures == 0 else 2


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="GreenFloor manager CLI")
    parser.add_argument("--program-config", default=_default_program_config_path())
    parser.add_argument("--markets-config", default=_default_markets_config_path())
    parser.add_argument("--state-db", default="")
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit compact single-line JSON output (default: pretty JSON).",
    )

    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("config-validate")

    p_onboard = sub.add_parser("keys-onboard")
    p_onboard.add_argument("--chia-keys-dir", default="")
    p_onboard.add_argument("--key-id", required=True)
    p_onboard.add_argument("--state-dir", default=".greenfloor/state")

    p_build_post = sub.add_parser("build-and-post-offer")
    group_market = p_build_post.add_mutually_exclusive_group(required=True)
    group_market.add_argument("--market-id", default="")
    group_market.add_argument(
        "--pair",
        default="",
        help="Pair selector in base:quote or base/quote form",
    )
    p_build_post.add_argument("--size-base-units", required=True, type=int)
    p_build_post.add_argument("--repeat", default=1, type=int)
    p_build_post.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_build_post.add_argument("--dexie-base-url", default="")
    p_build_post.add_argument(
        "--allow-take",
        action="store_true",
        help="Set drop_only=false when posting",
    )
    p_build_post.add_argument("--claim-rewards", action="store_true")
    p_build_post.add_argument("--dry-run", action="store_true")
    p_build_post.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_build_post.add_argument("--splash-base-url", default="")

    sub.add_parser("doctor")

    p_offers_status = sub.add_parser("offers-status")
    p_offers_status.add_argument("--market-id", default="")
    p_offers_status.add_argument("--limit", type=int, default=50)
    p_offers_status.add_argument("--events-limit", type=int, default=30)

    p_offers_reconcile = sub.add_parser("offers-reconcile")
    p_offers_reconcile.add_argument("--market-id", default="")
    p_offers_reconcile.add_argument("--limit", type=int, default=200)
    p_offers_reconcile.add_argument("--venue", choices=["dexie", "splash"], default=None)

    p_offers_cancel = sub.add_parser("offers-cancel")
    p_offers_cancel.add_argument("--offer-id", action="append", default=[])
    p_offers_cancel.add_argument("--cancel-open", action="store_true")

    p_bootstrap = sub.add_parser("bootstrap-home")
    p_bootstrap.add_argument("--home-dir", default="~/.greenfloor")
    p_bootstrap.add_argument("--program-template", default="config/program.yaml")
    p_bootstrap.add_argument("--markets-template", default="config/markets.yaml")
    p_bootstrap.add_argument("--force", action="store_true")

    p_coins_list = sub.add_parser("coins-list")
    p_coins_list.add_argument("--asset", default="")
    p_coins_list.add_argument("--vault-id", default="")

    p_coin_split = sub.add_parser("coin-split")
    split_market_group = p_coin_split.add_mutually_exclusive_group(required=True)
    split_market_group.add_argument("--market-id", default="")
    split_market_group.add_argument("--pair", default="")
    p_coin_split.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_coin_split.add_argument("--coin-id", action="append", default=[])
    p_coin_split.add_argument("--amount-per-coin", default=0, type=int)
    p_coin_split.add_argument("--number-of-coins", default=0, type=int)
    p_coin_split.add_argument("--size-base-units", default=0, type=int)
    p_coin_split.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_coin_split.add_argument("--until-ready", action="store_true")
    p_coin_split.add_argument("--max-iterations", default=3, type=int)
    p_coin_split.add_argument("--no-wait", action="store_true")

    p_coin_combine = sub.add_parser("coin-combine")
    combine_market_group = p_coin_combine.add_mutually_exclusive_group(required=True)
    combine_market_group.add_argument("--market-id", default="")
    combine_market_group.add_argument("--pair", default="")
    p_coin_combine.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_coin_combine.add_argument(
        "--input-coin-count",
        dest="input_coin_count",
        default=0,
        type=int,
        help="Number of input coins to combine.",
    )
    p_coin_combine.add_argument("--asset-id", default="")
    p_coin_combine.add_argument("--coin-id", action="append", default=[])
    p_coin_combine.add_argument("--size-base-units", default=0, type=int)
    p_coin_combine.add_argument("--venue", choices=["dexie", "splash"], default=None)
    p_coin_combine.add_argument("--until-ready", action="store_true")
    p_coin_combine.add_argument("--max-iterations", default=3, type=int)
    p_coin_combine.add_argument("--no-wait", action="store_true")

    args = parser.parse_args()
    global _JSON_OUTPUT_COMPACT
    _JSON_OUTPUT_COMPACT = bool(args.json)
    if args.command == "config-validate":
        code = _validate(Path(args.program_config), Path(args.markets_config))
    elif args.command == "keys-onboard":
        code = _keys_onboard(
            program_path=Path(args.program_config),
            key_id=args.key_id,
            state_dir=Path(args.state_dir),
            chia_keys_dir=Path(args.chia_keys_dir).expanduser()
            if str(args.chia_keys_dir).strip()
            else None,
        )
    elif args.command == "build-and-post-offer":
        venue, dexie_base_url, splash_base_url = _resolve_offer_publish_settings(
            program_path=Path(args.program_config),
            network=args.network,
            venue_override=args.venue,
            dexie_base_url=args.dexie_base_url or None,
            splash_base_url=args.splash_base_url or None,
        )
        code = _build_and_post_offer(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            size_base_units=args.size_base_units,
            repeat=args.repeat,
            publish_venue=venue,
            dexie_base_url=dexie_base_url,
            splash_base_url=splash_base_url,
            drop_only=not bool(args.allow_take),
            claim_rewards=bool(args.claim_rewards),
            dry_run=bool(args.dry_run),
        )
    elif args.command == "doctor":
        code = _doctor(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            state_db=args.state_db or None,
        )
    elif args.command == "offers-status":
        code = _offers_status(
            program_path=Path(args.program_config),
            state_db=args.state_db or None,
            market_id=args.market_id or None,
            limit=int(args.limit),
            events_limit=int(args.events_limit),
        )
    elif args.command == "offers-reconcile":
        code = _offers_reconcile(
            program_path=Path(args.program_config),
            state_db=args.state_db or None,
            market_id=args.market_id or None,
            limit=int(args.limit),
            venue=args.venue,
        )
    elif args.command == "offers-cancel":
        code = _offers_cancel(
            program_path=Path(args.program_config),
            offer_ids=[str(value) for value in args.offer_id],
            cancel_open=bool(args.cancel_open),
        )
    elif args.command == "bootstrap-home":
        code = _bootstrap_home(
            home_dir=Path(args.home_dir),
            program_template=Path(args.program_template),
            markets_template=Path(args.markets_template),
            force=bool(args.force),
        )
    elif args.command == "coins-list":
        code = _coins_list(
            program_path=Path(args.program_config),
            asset=args.asset or None,
            vault_id=args.vault_id or None,
        )
    elif args.command == "coin-split":
        code = _coin_split(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            coin_ids=[str(value) for value in args.coin_id],
            amount_per_coin=int(args.amount_per_coin),
            number_of_coins=int(args.number_of_coins),
            no_wait=bool(args.no_wait),
            venue=args.venue,
            size_base_units=int(args.size_base_units) or None,
            until_ready=bool(args.until_ready),
            max_iterations=int(args.max_iterations),
        )
    elif args.command == "coin-combine":
        code = _coin_combine(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            number_of_coins=int(args.input_coin_count),
            asset_id=args.asset_id or None,
            coin_ids=[str(value) for value in args.coin_id],
            no_wait=bool(args.no_wait),
            venue=args.venue,
            size_base_units=int(args.size_base_units) or None,
            until_ready=bool(args.until_ready),
            max_iterations=int(args.max_iterations),
        )
    else:
        raise ValueError(f"unsupported command: {args.command}")
    raise SystemExit(code)


if __name__ == "__main__":
    main()
