from __future__ import annotations

import argparse
import asyncio
import concurrent.futures
import contextlib
import fcntl
import json
import logging
import os
import threading
import time
import urllib.parse
from collections import deque
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import yaml  # noqa: F401

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import (
    CoinsetAdapter,
    extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
)
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter, XchPriceProvider
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import (
    default_cats_config_path,
    default_state_dir_path,
    load_markets_config_with_optional_overlay,
    load_program_config,
    resolve_quote_asset_for_offer,
    resolve_trade_asset_for_dexie,
)
from greenfloor.core.coin_ops import BucketSpec, CoinOpPlan, plan_coin_ops
from greenfloor.core.fee_budget import partition_plans_by_budget, projected_coin_ops_fee_mojos
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert, utcnow
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.core.strategy import MarketState, PlannedAction, StrategyConfig, evaluate_market
from greenfloor.daemon.cloud_wallet_list_cache import CloudWalletAssetScopedListCache
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient, capture_coinset_websocket_once
from greenfloor.daemon.reservations import AssetReservationCoordinator
from greenfloor.hex_utils import default_mojo_multiplier_for_asset, is_hex_id
from greenfloor.keys.router import resolve_market_key
from greenfloor.logging_setup import (
    initialize_service_file_logging,
    warn_if_log_level_auto_healed,
)
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.runtime.offer_execution import (
    build_and_post_offer_cloud_wallet,
    is_transient_dexie_visibility_404_error,
    resolve_cloud_wallet_offer_asset_ids,
)
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState

_DEFAULT_CANCEL_MOVE_THRESHOLD_BPS = 500
_POST_COOLDOWN_UNTIL: dict[str, float] = {}
_CANCEL_COOLDOWN_UNTIL: dict[str, float] = {}
_COOLDOWN_LOCK = threading.Lock()
_DAEMON_SERVICE_NAME = "daemon"
_daemon_logger = logging.getLogger("greenfloor.daemon")
_DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT = 3600
_DISABLED_MARKET_NEXT_LOG_AT: dict[str, float] = {}
_DISABLED_MARKET_STARTUP_LOGGED = False
_WATCHED_COIN_IDS_BY_MARKET: dict[str, set[str]] = {}
_WATCHED_COIN_IDS_LOCK = threading.Lock()
_CLOUD_WALLET_SPENDABLE_STATES = {
    "CONFIRMED",
    "UNSPENT",
    "SPENDABLE",
    "AVAILABLE",
    "SETTLED",
}
_GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET = 3
_GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS = 60


def _cloud_wallet_coin_matches_asset_scope(*, coin: dict[str, Any], scoped_asset_id: str) -> bool:
    target_asset = str(scoped_asset_id).strip().lower()
    if not target_asset:
        return False
    asset_payload = coin.get("asset")
    if isinstance(asset_payload, dict):
        coin_asset_id = str(asset_payload.get("id", "")).strip().lower()
        if coin_asset_id:
            return coin_asset_id == target_asset
    # Asset-scoped Cloud Wallet coin queries can omit per-row asset metadata.
    # When that happens, trust the requested scope rather than discarding rows.
    return True


def _coin_op_min_amount_mojos(*, canonical_asset_id: str) -> int:
    if str(canonical_asset_id).strip().lower() == "xch":
        return 0
    return 1000


def _coin_meets_coin_op_min_amount(coin: dict[str, Any], *, canonical_asset_id: str) -> bool:
    try:
        amount = int(coin.get("amount", 0))
    except (TypeError, ValueError):
        return False
    return amount >= _coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)


def _coin_op_target_amount_allowed(*, amount_mojos: int, canonical_asset_id: str) -> bool:
    return int(amount_mojos) >= _coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)


def _coin_matches_direct_spendable_lookup(
    *,
    wallet: Any,
    coin: dict[str, Any],
    scoped_asset_id: str,
    cache: dict[str, bool] | None = None,
) -> bool:
    get_coin_record = getattr(wallet, "get_coin_record", None)
    if not callable(get_coin_record):
        return True
    coin_id = str(coin.get("id", "")).strip()
    if not coin_id:
        return False
    if cache is not None and coin_id in cache:
        return bool(cache[coin_id])
    # Temporary upstream defense: asset-scoped Cloud Wallet coin queries can
    # leak cross-asset rows into CAT inventories. Re-check the exact coin
    # record before coin-op selection until upstream fixes the scoped query.
    fallback_result = bool(
        str(coin.get("state", "")).strip().upper() in _CLOUD_WALLET_SPENDABLE_STATES
        and not bool(coin.get("isLocked"))
    )
    try:
        coin_record = get_coin_record(coin_id=coin_id)
    except Exception:
        # Fail-open on lookup errors so transient Cloud Wallet read timeouts do
        # not collapse scoped inventories to zero.
        result = fallback_result
    else:
        if not isinstance(coin_record, dict):
            result = fallback_result
        else:
            state = str(coin_record.get("state", coin.get("state", ""))).strip().upper()
            asset_payload = coin_record.get("asset")
            asset_id = (
                str(asset_payload.get("id", "")).strip().lower()
                if isinstance(asset_payload, dict)
                else ""
            )
            base_match = bool(
                state in _CLOUD_WALLET_SPENDABLE_STATES
                and not bool(coin_record.get("isLocked"))
                and not bool(coin_record.get("isLinkedToOpenOffer"))
            )
            # Some coin-record lookups omit asset metadata despite scoped query
            # context. When asset id is missing, trust the scoped list row.
            result = base_match and (
                asset_id == str(scoped_asset_id).strip().lower() if asset_id else True
            )
    if cache is not None:
        cache[coin_id] = result
    return result


_DAEMON_INSTANCE_LOCK_FILENAME = "daemon.lock"


def _log_market_decision(market_id: str, decision: str, **fields: Any) -> None:
    extras = " ".join(f"{key}={fields[key]}" for key in sorted(fields))
    if extras:
        _daemon_logger.info(
            "market_decision market_id=%s decision=%s %s", market_id, decision, extras
        )
    else:
        _daemon_logger.info("market_decision market_id=%s decision=%s", market_id, decision)


def _log_offer_action_timing(market_id: str, item: dict[str, Any]) -> None:
    if any(
        item.get(k) is not None for k in ("offer_create_ms", "offer_publish_ms", "offer_total_ms")
    ):
        _log_market_decision(
            market_id,
            "offer_action_timing",
            size=int(item.get("size", 0) or 0),
            side=str(item.get("side", "sell")),
            status=str(item.get("status", "")),
            reason=str(item.get("reason", "")),
            offer_id=str(item.get("offer_id", "") or ""),
            offer_create_ms=item.get("offer_create_ms"),
            offer_publish_ms=item.get("offer_publish_ms"),
            offer_total_ms=item.get("offer_total_ms"),
            offer_create_phase_ms=item.get("offer_create_phase_ms"),
            offer_artifact_wait_ms=item.get("offer_artifact_wait_ms"),
        )


def _initialize_daemon_file_logging(home_dir: str, *, log_level: str | None) -> None:
    initialize_service_file_logging(
        service_name=_DAEMON_SERVICE_NAME,
        home_dir=home_dir,
        log_level=log_level,
        service_logger=_daemon_logger,
        allow_reinit_level=True,
    )


def _disabled_market_log_interval_seconds() -> int:
    return _env_int(
        "GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS",
        _DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT,
        minimum=60,
    )


def _should_log_disabled_market(*, market_id: str, now_monotonic: float | None = None) -> bool:
    now_value = time.monotonic() if now_monotonic is None else float(now_monotonic)
    deadline = float(_DISABLED_MARKET_NEXT_LOG_AT.get(market_id, 0.0))
    if deadline > now_value:
        return False
    _DISABLED_MARKET_NEXT_LOG_AT[market_id] = now_value + float(
        _disabled_market_log_interval_seconds()
    )
    return True


def _log_disabled_markets_startup_once(*, markets: list[Any]) -> None:
    global _DISABLED_MARKET_STARTUP_LOGGED
    if _DISABLED_MARKET_STARTUP_LOGGED:
        return
    interval_seconds = _disabled_market_log_interval_seconds()
    disabled_market_ids = [
        str(getattr(market, "market_id", "")).strip()
        for market in markets
        if not bool(getattr(market, "enabled", True))
    ]
    disabled_market_ids = [market_id for market_id in disabled_market_ids if market_id]
    if disabled_market_ids:
        _daemon_logger.info(
            "disabled_markets_startup count=%s interval_seconds=%s market_ids=%s",
            len(disabled_market_ids),
            interval_seconds,
            sorted(disabled_market_ids),
        )
        now_value = time.monotonic()
        for market_id in disabled_market_ids:
            _DISABLED_MARKET_NEXT_LOG_AT[market_id] = now_value + float(interval_seconds)
    _DISABLED_MARKET_STARTUP_LOGGED = True


def _warn_if_log_level_auto_healed(*, program, program_path: Path) -> None:
    warn_if_log_level_auto_healed(
        program_obj=program, program_path=program_path, logger=_daemon_logger
    )


def _log_daemon_event(*, level: int, payload: dict[str, Any]) -> None:
    _daemon_logger.log(level, "daemon_event %s", json.dumps(payload, sort_keys=True))


def _consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


def _daemon_instance_lock_path(*, state_dir: Path) -> Path:
    return state_dir / _DAEMON_INSTANCE_LOCK_FILENAME


@contextlib.contextmanager
def _acquire_daemon_instance_lock(*, state_dir: Path, mode: str):
    state_dir.mkdir(parents=True, exist_ok=True)
    lock_path = _daemon_instance_lock_path(state_dir=state_dir)
    lock_file = lock_path.open("a+", encoding="utf-8")
    try:
        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            existing = ""
            try:
                lock_file.seek(0)
                existing = lock_file.read().strip()
            except Exception:
                existing = ""
            detail = f" daemon_lock_metadata={existing}" if existing else ""
            raise RuntimeError(f"daemon_already_running:{lock_path}{detail}") from exc
        payload = {
            "pid": os.getpid(),
            "mode": str(mode).strip(),
            "acquired_at": datetime.now(UTC).isoformat(),
        }
        lock_file.seek(0)
        lock_file.truncate()
        lock_file.write(json.dumps(payload, sort_keys=True))
        lock_file.flush()
        yield
    finally:
        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)
        except Exception:
            pass
        lock_file.close()


def _resolve_db_path(program_home_dir: str, explicit_db_path: str | None) -> Path:
    if explicit_db_path:
        return Path(explicit_db_path).expanduser()
    return (Path(program_home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()


def _cancel_move_threshold_bps(*, market: Any | None = None) -> int:
    pricing = dict(getattr(market, "pricing", {}) or {}) if market is not None else {}
    threshold_raw = pricing.get("cancel_move_threshold_bps")
    if threshold_raw is not None:
        try:
            parsed_threshold = int(threshold_raw)
        except (TypeError, ValueError):
            parsed_threshold = 0
        if parsed_threshold > 0:
            return parsed_threshold
    raw = os.getenv("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", "").strip()
    if not raw:
        return _DEFAULT_CANCEL_MOVE_THRESHOLD_BPS
    try:
        parsed = int(raw)
    except ValueError:
        return _DEFAULT_CANCEL_MOVE_THRESHOLD_BPS
    return max(1, parsed)


def _abs_move_bps(current: float | None, previous: float | None) -> float | None:
    if current is None or previous is None:
        return None
    if current <= 0 or previous <= 0:
        return None
    return abs((current - previous) / previous) * 10_000.0


def _env_int(name: str, default: int, minimum: int = 0) -> int:
    raw = os.getenv(name, "").strip()
    if not raw:
        return default
    try:
        value = int(raw)
    except ValueError:
        return default
    return max(minimum, value)


def _post_retry_config() -> tuple[int, int, int]:
    attempts = _env_int("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", 2, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_OFFER_POST_BACKOFF_MS", 250, minimum=0)
    cooldown_seconds = _env_int("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", 30, minimum=0)
    return attempts, backoff_ms, cooldown_seconds


def _cancel_retry_config() -> tuple[int, int, int]:
    attempts = _env_int("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", 2, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", 250, minimum=0)
    cooldown_seconds = _env_int("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", 30, minimum=0)
    return attempts, backoff_ms, cooldown_seconds


def _combine_retry_config() -> tuple[int, int]:
    attempts = _env_int("GREENFLOOR_COIN_OPS_COMBINE_MAX_ATTEMPTS", 3, minimum=1)
    backoff_ms = _env_int("GREENFLOOR_COIN_OPS_COMBINE_BACKOFF_MS", 1000, minimum=0)
    return attempts, backoff_ms


def _combine_input_coin_cap() -> int:
    # Keep CAT parent lookup fan-out bounded when Cloud Wallet resolves many input coins.
    return _env_int("GREENFLOOR_COIN_OPS_COMBINE_INPUT_COIN_CAP", 5, minimum=2)


def _is_cloud_wallet_rate_limited_error(exc: Exception) -> bool:
    text = str(exc).strip().lower()
    return "status not ok: 429" in text or " 429" in text or text.endswith(":429")


def _is_transient_cloud_wallet_upstream_error_text(error_text: str) -> bool:
    normalized = str(error_text or "").strip().lower()
    if CloudWalletAdapter._is_transient_error_message(normalized):
        return True
    return any(
        marker in normalized
        for marker in (
            "cloud_wallet_http_error:502",
            "cloud_wallet_http_error:503",
            "cloud_wallet_http_error:504",
            "cloud_wallet_network_error",
        )
    )


def _cloud_wallet_reason_is_503(reason_text: str) -> bool:
    normalized = str(reason_text or "").strip().lower()
    return (
        "cloud_wallet_http_error:503" in normalized
        or "503 service temporarily unavailable" in normalized
    )


def _cloud_wallet_item_is_success(item: dict[str, Any]) -> bool:
    status = str(item.get("status", "")).strip().lower()
    reason = str(item.get("reason", "")).strip().lower()
    return status == "executed" and (
        reason == "cloud_wallet_post_success" or reason == _PENDING_VISIBILITY_REASON.lower()
    )


def _parse_iso_datetime(value: str) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        return datetime.fromisoformat(text.replace("Z", "+00:00"))
    except ValueError:
        return None


@dataclass(slots=True)
class _CWHealthSnapshot:
    count_503: int
    had_success: bool
    timestamp: datetime


_CLOUD_WALLET_HEALTH_WINDOW: dict[str, deque[_CWHealthSnapshot]] = {}


def _cloud_wallet_market_health_payload(
    *,
    market_id: str,
    current_items: list[dict[str, Any]],
    now: datetime,
    window_size: int = 40,
) -> dict[str, Any]:
    window = _CLOUD_WALLET_HEALTH_WINDOW.setdefault(
        str(market_id), deque(maxlen=max(1, window_size))
    )
    batch_503 = sum(
        1 for item in current_items if _cloud_wallet_reason_is_503(str(item.get("reason", "")))
    )
    batch_success = any(_cloud_wallet_item_is_success(item) for item in current_items)
    window.append(_CWHealthSnapshot(count_503=batch_503, had_success=batch_success, timestamp=now))

    rolling_503_count = sum(s.count_503 for s in window)
    last_success_at: str | None = None
    for s in reversed(window):
        if s.had_success:
            last_success_at = s.timestamp.isoformat()
            break
    last_success_age_seconds: int | None = None
    if last_success_at is not None:
        parsed = _parse_iso_datetime(last_success_at)
        if parsed is not None:
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=UTC)
            last_success_age_seconds = max(0, int((now - parsed).total_seconds()))
    return {
        "market_id": str(market_id),
        "rolling_window_events": len(window),
        "rolling_503_count": int(rolling_503_count),
        "last_cloud_wallet_success_at": last_success_at,
        "last_cloud_wallet_success_age_seconds": last_success_age_seconds,
    }


def _combine_coins_with_retry(
    *,
    cloud_wallet: CloudWalletAdapter,
    combine_kwargs: dict[str, Any],
) -> dict[str, Any]:
    attempts_max, backoff_ms = _combine_retry_config()
    last_exc: Exception | None = None
    for attempt in range(1, attempts_max + 1):
        try:
            return cloud_wallet.combine_coins(**combine_kwargs)
        except Exception as exc:
            last_exc = exc
            if attempt >= attempts_max or not _is_cloud_wallet_rate_limited_error(exc):
                raise
            if backoff_ms > 0:
                time.sleep((backoff_ms * (2 ** (attempt - 1))) / 1000.0)
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("combine_coins_failed_without_exception")


def _cooldown_remaining_ms(cooldowns: dict[str, float], key: str) -> int:
    with _COOLDOWN_LOCK:
        deadline = float(cooldowns.get(key, 0.0))
    remaining = max(0.0, deadline - time.monotonic())
    return int(remaining * 1000)


def _set_cooldown(cooldowns: dict[str, float], key: str, cooldown_seconds: int) -> None:
    if cooldown_seconds <= 0:
        return
    with _COOLDOWN_LOCK:
        cooldowns[key] = time.monotonic() + float(cooldown_seconds)


def _retry_with_backoff(
    *,
    action_fn: Callable[[], dict[str, Any]],
    is_success: Callable[[dict[str, Any]], bool],
    default_error: str,
    retry_config: tuple[int, int, int],
) -> tuple[dict[str, Any], int, str]:
    """Generic retry loop with exponential backoff."""
    attempts_max, backoff_ms, _ = retry_config
    last_error = default_error
    for attempt in range(1, attempts_max + 1):
        try:
            result = action_fn()
        except Exception as exc:
            result = {"success": False, "error": f"{default_error}:{exc}"}
        if is_success(result):
            return result, attempt, ""
        last_error = str(result.get("error", default_error))
        if attempt < attempts_max and backoff_ms > 0:
            time.sleep((backoff_ms * (2 ** (attempt - 1))) / 1000.0)
    return {"success": False, "error": last_error}, attempts_max, last_error


def _is_venue_post_success(result: dict[str, Any]) -> bool:
    return bool(result.get("success", False)) and bool(str(result.get("id", "")).strip())


def _is_cancel_success(result: dict[str, Any]) -> bool:
    return bool(result.get("success", False))


def _post_offer_with_retry(
    *,
    publish_venue: str,
    offer_text: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
) -> tuple[dict[str, Any], int, str]:
    def _do_post() -> dict[str, Any]:
        if publish_venue == "splash":
            if splash is None:
                return {"success": False, "error": "splash_not_configured"}
            return splash.post_offer(offer_text)
        return dexie.post_offer(offer_text)

    return _retry_with_backoff(
        action_fn=_do_post,
        is_success=_is_venue_post_success,
        default_error=f"{publish_venue}_post_failed",
        retry_config=_post_retry_config(),
    )


def _cancel_offer_with_retry(
    *,
    dexie: DexieAdapter,
    offer_id: str,
) -> tuple[dict[str, Any], int, str]:
    return _retry_with_backoff(
        action_fn=lambda: dexie.cancel_offer(offer_id),
        is_success=_is_cancel_success,
        default_error="cancel_offer_failed",
        retry_config=_cancel_retry_config(),
    )


def _normalize_strategy_pair(quote_asset: str) -> str:
    lowered = quote_asset.strip().lower()
    if lowered == "xch":
        return "xch"
    if "usdc" in lowered:
        return "usdc"
    return lowered


def _is_hex_asset_id(value: str) -> bool:
    return is_hex_id(value)


def _default_cats_config_path() -> Path | None:
    return default_cats_config_path()


def _resolve_quote_asset_for_offer(*, quote_asset: str, network: str) -> str:
    return resolve_quote_asset_for_offer(quote_asset=quote_asset, network=network)


def _market_pricing(market: Any) -> dict[str, Any]:
    return dict(getattr(market, "pricing", {}) or {})


def _normalize_target_counts(
    raw: dict,
    *,
    defaults: dict[int, int] | None = None,
) -> dict[int, int]:
    """Normalize a {size: target_count} mapping from config or ladder data.

    Drops non-positive sizes, clamps negative targets to zero, and falls back
    to *defaults* when the result would otherwise be empty.
    """
    out = {int(k): max(0, int(v)) for k, v in raw.items() if int(k) > 0}
    if not out and defaults:
        return dict(defaults)
    return out


def _strategy_config_from_market(market) -> StrategyConfig:
    sell_ladder = market.ladders.get("sell", [])
    targets_by_size = {int(e.size_base_units): int(e.target_count) for e in sell_ladder}
    pricing = _market_pricing(market)

    def _to_int(value: Any) -> int | None:
        if value is None:
            return None
        try:
            parsed = int(value)
        except (TypeError, ValueError):
            return None
        return parsed

    def _to_float(value: Any) -> float | None:
        if value is None:
            return None
        try:
            parsed = float(value)
        except (TypeError, ValueError):
            return None
        return parsed

    normalized_targets = _normalize_target_counts(targets_by_size, defaults={1: 5, 10: 2, 100: 1})

    return StrategyConfig(
        pair=_normalize_strategy_pair(market.quote_asset),
        ones_target=int(normalized_targets.get(1, 0)),
        tens_target=int(normalized_targets.get(10, 0)),
        hundreds_target=int(normalized_targets.get(100, 0)),
        target_spread_bps=_to_int(pricing.get("strategy_target_spread_bps")),
        min_xch_price_usd=_to_float(pricing.get("strategy_min_xch_price_usd")),
        max_xch_price_usd=_to_float(pricing.get("strategy_max_xch_price_usd")),
        offer_expiry_minutes=_to_int(pricing.get("strategy_offer_expiry_minutes")),
        target_counts_by_size=normalized_targets,
    )


def _strategy_config_for_side(*, market: Any, side: str) -> StrategyConfig:
    ladders = getattr(market, "ladders", {}) or {}
    side_ladder = list(ladders.get(side, []) or []) if isinstance(ladders, dict) else []
    targets_by_size = {int(e.size_base_units): int(e.target_count) for e in side_ladder}
    pricing = _market_pricing(market)

    expiry_minutes_raw = pricing.get("strategy_offer_expiry_minutes")
    expiry_minutes: int | None = None
    if expiry_minutes_raw is not None:
        try:
            expiry_minutes = int(expiry_minutes_raw)
        except (TypeError, ValueError):
            expiry_minutes = None

    normalized_targets = _normalize_target_counts(targets_by_size)

    return StrategyConfig(
        pair=_normalize_strategy_pair(market.quote_asset),
        ones_target=int(normalized_targets.get(1, 0)),
        tens_target=int(normalized_targets.get(10, 0)),
        hundreds_target=int(normalized_targets.get(100, 0)),
        offer_expiry_minutes=expiry_minutes,
        target_counts_by_size=normalized_targets,
    )


def _strategy_state_from_bucket_counts(
    bucket_counts: dict[int, int],
    *,
    xch_price_usd: float | None,
) -> MarketState:
    normalized_bucket_counts = {int(size): int(count) for size, count in bucket_counts.items()}
    return MarketState(
        ones=int(normalized_bucket_counts.get(1, 0)),
        tens=int(normalized_bucket_counts.get(10, 0)),
        hundreds=int(normalized_bucket_counts.get(100, 0)),
        xch_price_usd=xch_price_usd,
        bucket_counts_by_size=normalized_bucket_counts,
    )


def _effective_sell_bucket_counts_for_coin_ops(
    *,
    sell_ladder: list[Any],
    wallet_bucket_counts: dict[int, int],
    active_sell_offer_counts_by_size: dict[int, int] | None,
    newly_executed_sell_offer_counts_by_size: dict[int, int] | None = None,
) -> dict[int, int]:
    effective_counts = dict(wallet_bucket_counts)
    active_sell_counts = active_sell_offer_counts_by_size or {}
    newly_executed_sell_counts = newly_executed_sell_offer_counts_by_size or {}
    for entry in sell_ladder:
        size_base_units = int(getattr(entry, "size_base_units", 0))
        if size_base_units <= 0:
            continue
        target_count = max(0, int(getattr(entry, "target_count", 0)))
        newly_executed_sell_count = max(0, int(newly_executed_sell_counts.get(size_base_units, 0)))
        wallet_count = max(
            0,
            int(wallet_bucket_counts.get(size_base_units, 0)) - newly_executed_sell_count,
        )
        active_sell_count = max(0, int(active_sell_counts.get(size_base_units, 0)))
        effective_active_sell_count = active_sell_count + newly_executed_sell_count
        # Count live sell offers toward the market target, but not toward the
        # split buffer. That preserves at most one extra ready coin above the
        # active sell ladder coverage.
        effective_counts[size_base_units] = wallet_count + min(
            effective_active_sell_count,
            target_count,
        )
    return effective_counts


def _executed_sell_offer_counts_by_size(offer_execution: dict[str, Any]) -> dict[int, int]:
    counts: dict[int, int] = {}
    items = offer_execution.get("items", [])
    if not isinstance(items, list):
        return counts
    for item in items:
        if not isinstance(item, dict):
            continue
        if str(item.get("status", "")).strip().lower() != "executed":
            continue
        if _normalize_offer_side(item.get("side", "sell")) != "sell":
            continue
        try:
            size = int(item.get("size", 0))
        except (TypeError, ValueError):
            continue
        if size <= 0:
            continue
        counts[size] = counts.get(size, 0) + 1
    return counts


def _evaluate_two_sided_market_actions(
    *,
    market: Any,
    counts_by_side: dict[str, dict[int, int]],
    xch_price_usd: float | None,
    now: datetime,
) -> list[PlannedAction]:
    actions: list[PlannedAction] = []
    for side in ("buy", "sell"):
        side_config = _strategy_config_for_side(market=market, side=side)
        side_state = _strategy_state_from_bucket_counts(
            counts_by_side.get(side, {}),
            xch_price_usd=xch_price_usd,
        )
        side_actions = evaluate_market(state=side_state, config=side_config, clock=now)
        actions.extend(
            PlannedAction(
                size=int(action.size),
                repeat=int(action.repeat),
                pair=action.pair,
                expiry_unit=action.expiry_unit,
                expiry_value=int(action.expiry_value),
                cancel_after_create=action.cancel_after_create,
                reason=action.reason,
                target_spread_bps=action.target_spread_bps,
                side=side,
            )
            for action in side_actions
        )
    return actions


_ACTIVE_OFFER_STATES_FOR_RESEED = {
    OfferLifecycleState.OPEN.value,
    OfferLifecycleState.REFRESH_DUE.value,
}
_RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60
_PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS = 2 * 60
_PENDING_VISIBILITY_REASON = "cloud_wallet_post_success_dexie_visibility_pending"


@dataclass(frozen=True, slots=True)
class _OfferExecutionMetadata:
    size: int
    side: str | None
    reason: str
    created_at: str


def _is_recent_mempool_observed_offer_state(
    *,
    offer_state: dict[str, Any],
    clock: datetime,
    max_age_seconds: int = _RESEED_MEMPOOL_MAX_AGE_SECONDS,
) -> bool:
    state = str(offer_state.get("state", "")).strip().lower()
    if state != OfferLifecycleState.MEMPOOL_OBSERVED.value:
        return False
    updated_at_raw = str(offer_state.get("updated_at", "")).strip()
    if not updated_at_raw:
        return False
    normalized = updated_at_raw.replace("Z", "+00:00")
    try:
        updated_at = datetime.fromisoformat(normalized)
    except ValueError:
        return False
    if updated_at.tzinfo is None:
        _daemon_logger.warning(
            "offer state timestamp missing timezone, assuming UTC: %s", updated_at_raw
        )
        updated_at = updated_at.replace(tzinfo=UTC)
    age_seconds = (clock - updated_at).total_seconds()
    return 0 <= age_seconds <= float(max_age_seconds)


def _strategy_target_counts_by_size(strategy_config: StrategyConfig) -> dict[int, int]:
    if strategy_config.target_counts_by_size:
        return {
            int(size): int(target)
            for size, target in sorted(strategy_config.target_counts_by_size.items())
            if int(size) > 0 and int(target) >= 0
        }
    return {
        1: int(strategy_config.ones_target),
        10: int(strategy_config.tens_target),
        100: int(strategy_config.hundreds_target),
    }


def _recent_offer_sizes_by_offer_id(*, store: SqliteStore, market_id: str) -> dict[str, int]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    size_by_offer_id: dict[str, int] = {}
    for event in events:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            offer_id = str(item.get("offer_id", "")).strip()
            if not offer_id:
                continue
            try:
                size = int(item.get("size") or 0)
            except (TypeError, ValueError):
                continue
            if size <= 0:
                continue
            # Events are returned newest-first; keep first (latest) mapping.
            if offer_id not in size_by_offer_id:
                size_by_offer_id[offer_id] = size
    return size_by_offer_id


def _normalize_offer_side(value: Any) -> str:
    side = str(value or "").strip().lower()
    return "buy" if side == "buy" else "sell"


def _parse_offer_side_metadata(value: Any) -> str | None:
    side = str(value or "").strip().lower()
    if side in {"buy", "sell"}:
        return side
    return None


def _recent_offer_metadata_by_offer_id(
    *, store: SqliteStore, market_id: str
) -> dict[str, _OfferExecutionMetadata]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    metadata_by_offer_id: dict[str, _OfferExecutionMetadata] = {}
    for event in events:
        created_at = str(event.get("created_at", "")).strip()
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            offer_id = str(item.get("offer_id", "")).strip()
            if not offer_id:
                continue
            try:
                size = int(item.get("size") or 0)
            except (TypeError, ValueError):
                continue
            if size <= 0:
                continue
            side = _parse_offer_side_metadata(item.get("side"))
            reason = str(item.get("reason", "")).strip()
            # Events are returned newest-first; keep first (latest) mapping.
            if offer_id not in metadata_by_offer_id:
                metadata_by_offer_id[offer_id] = _OfferExecutionMetadata(
                    size=size,
                    side=side,
                    reason=reason,
                    created_at=created_at,
                )
    return metadata_by_offer_id


def _parse_event_created_at(value: Any) -> datetime | None:
    raw = str(value or "").strip()
    if not raw:
        return None
    normalized = raw.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed


def _expiry_seconds_for_action(action: PlannedAction) -> int | None:
    unit = str(action.expiry_unit or "").strip().lower()
    try:
        value = int(action.expiry_value)
    except (TypeError, ValueError):
        return None
    if value <= 0:
        return None
    unit_seconds = {
        "second": 1,
        "seconds": 1,
        "minute": 60,
        "minutes": 60,
        "hour": 60 * 60,
        "hours": 60 * 60,
        "day": 24 * 60 * 60,
        "days": 24 * 60 * 60,
    }.get(unit)
    if unit_seconds is None:
        return None
    return value * unit_seconds


def _apply_action_cadence_gate(
    *,
    actions: list[PlannedAction],
    target_counts_by_side: dict[str, dict[int, int]],
    active_counts_by_side: dict[str, dict[int, int]],
    store: SqliteStore,
    market_id: str,
    clock: datetime,
) -> tuple[list[PlannedAction], list[dict[str, Any]]]:
    _ = target_counts_by_side, active_counts_by_side, store, market_id, clock
    passthrough_actions = [action for action in actions if int(action.repeat) > 0]
    return passthrough_actions, []


def _is_stale_pending_visibility_offer(
    *,
    offer_id: str,
    metadata: _OfferExecutionMetadata,
    dexie_size_by_offer_id: dict[str, int] | None,
    clock: datetime,
    max_age_seconds: int = _PENDING_VISIBILITY_RECHECK_MAX_AGE_SECONDS,
) -> bool:
    if metadata.reason != _PENDING_VISIBILITY_REASON:
        return False
    if dexie_size_by_offer_id is None:
        # No Dexie visibility snapshot available this cycle.
        return False
    if offer_id in dexie_size_by_offer_id:
        return False
    created_at_raw = str(metadata.created_at).strip()
    if not created_at_raw:
        return True
    normalized = created_at_raw.replace("Z", "+00:00")
    try:
        created_at = datetime.fromisoformat(normalized)
    except ValueError:
        return True
    if created_at.tzinfo is None:
        created_at = created_at.replace(tzinfo=UTC)
    return (clock - created_at).total_seconds() > float(max_age_seconds)


def _is_dexie_offer_missing_error(error: Exception) -> bool:
    raw = str(error).strip()
    if not raw:
        return False
    normalized = raw.lower()
    return is_transient_dexie_visibility_404_error(raw) or (
        "http error 404" in normalized and "not found" in normalized
    )


def _recent_executed_offer_ids(*, store: SqliteStore, market_id: str) -> set[str]:
    events = store.list_recent_audit_events(
        event_types=["strategy_offer_execution"],
        market_id=market_id,
        limit=1500,
    )
    offer_ids: set[str] = set()
    for event in events:
        payload = event.get("payload")
        if not isinstance(payload, dict):
            continue
        single_offer_id = str(payload.get("offer_id", "")).strip()
        if single_offer_id:
            offer_ids.add(single_offer_id)
        items = payload.get("items")
        if not isinstance(items, list):
            continue
        for item in items:
            if not isinstance(item, dict):
                continue
            if str(item.get("status", "")).strip().lower() != "executed":
                continue
            item_offer_id = str(item.get("offer_id", "")).strip()
            if item_offer_id:
                offer_ids.add(item_offer_id)
    return offer_ids


def _watchlist_offer_ids_from_store(
    *, store: SqliteStore, market_id: str, clock: datetime
) -> set[str]:
    tracked_states = {
        OfferLifecycleState.OPEN.value,
        OfferLifecycleState.REFRESH_DUE.value,
        "unknown_orphaned",
    }
    offer_ids: set[str] = set()
    for item in store.list_offer_states(market_id=market_id, limit=500):
        state = str(item.get("state", "")).strip().lower()
        offer_id = str(item.get("offer_id", "")).strip()
        if not offer_id:
            continue
        if state in tracked_states or _is_recent_mempool_observed_offer_state(
            offer_state=item, clock=clock
        ):
            offer_ids.add(offer_id)
    return offer_ids


def _set_watched_coin_ids_for_market(*, market_id: str, coin_ids: set[str]) -> None:
    with _WATCHED_COIN_IDS_LOCK:
        _WATCHED_COIN_IDS_BY_MARKET[market_id] = set(coin_ids)


def _match_watched_coin_ids(*, observed_coin_ids: list[str]) -> dict[str, list[str]]:
    normalized = {
        str(coin_id).strip().lower() for coin_id in observed_coin_ids if str(coin_id).strip()
    }
    if not normalized:
        return {}
    matches: dict[str, list[str]] = {}
    with _WATCHED_COIN_IDS_LOCK:
        for market_id, watched in _WATCHED_COIN_IDS_BY_MARKET.items():
            intersection = sorted(normalized.intersection(watched))
            if intersection:
                matches[market_id] = intersection
    return matches


def _watched_coin_ids_for_market(*, market_id: str) -> set[str]:
    with _WATCHED_COIN_IDS_LOCK:
        return set(_WATCHED_COIN_IDS_BY_MARKET.get(market_id, set()))


def _update_market_coin_watchlist_from_dexie(
    *,
    market,
    offers: list[dict[str, Any]],
    store: SqliteStore,
    clock: datetime,
) -> None:
    watch_offer_ids = _watchlist_offer_ids_from_store(
        store=store,
        market_id=market.market_id,
        clock=clock,
    )
    watch_offer_ids.update(_recent_executed_offer_ids(store=store, market_id=market.market_id))
    watched_coin_ids: set[str] = set()
    matched_offer_count = 0
    for offer in offers:
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id or offer_id not in watch_offer_ids:
            continue
        matched_offer_count += 1
        watched_coin_ids.update(extract_coin_ids_from_offer_payload(offer))
    _set_watched_coin_ids_for_market(market_id=market.market_id, coin_ids=watched_coin_ids)
    store.add_audit_event(
        "coin_watchlist_updated",
        {
            "market_id": market.market_id,
            "watch_offer_count": len(watch_offer_ids),
            "matched_offer_count": matched_offer_count,
            "watch_coin_count": len(watched_coin_ids),
            "watch_coin_sample": sorted(watched_coin_ids)[:10],
        },
        market_id=market.market_id,
    )


def _build_dexie_size_by_offer_id(
    offers: list[dict[str, Any]], base_asset_id: str
) -> dict[str, int]:
    """Extract {offer_id -> size_base_units} from a list of flat Dexie offer dicts.

    Works with both the list endpoint (each element is a flat offer dict) and a
    single element extracted from a get_offer() response (payload["offer"]).
    """
    result: dict[str, int] = {}
    clean_base = str(base_asset_id).strip().lower()
    for offer in offers:
        if not isinstance(offer, dict):
            continue
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        for offered_item in offer.get("offered") or []:
            if not isinstance(offered_item, dict):
                continue
            if str(offered_item.get("id", "")).strip().lower() != clean_base:
                continue
            try:
                size = int(offered_item["amount"])
            except (TypeError, ValueError, KeyError):
                continue
            if size > 0:
                result[offer_id] = size
    return result


def _active_offer_state_summary(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
) -> tuple[list[str], dict[str, int], dict[str, _OfferExecutionMetadata]]:
    offer_states = store.list_offer_states(market_id=market_id, limit=limit)
    state_counts: dict[str, int] = {}
    for item in offer_states:
        state = str(item.get("state", "")).strip().lower()
        if not state:
            continue
        state_counts[state] = int(state_counts.get(state, 0)) + 1
    active_offer_ids: list[str] = []
    for item in offer_states:
        state = str(item.get("state", "")).strip().lower()
        if state in _ACTIVE_OFFER_STATES_FOR_RESEED:
            active_offer_id = str(item.get("offer_id", "")).strip()
            if active_offer_id:
                active_offer_ids.append(active_offer_id)
            continue
        if _is_recent_mempool_observed_offer_state(offer_state=item, clock=clock):
            active_offer_id = str(item.get("offer_id", "")).strip()
            if active_offer_id:
                active_offer_ids.append(active_offer_id)
    return (
        active_offer_ids,
        state_counts,
        _recent_offer_metadata_by_offer_id(store=store, market_id=market_id),
    )


def _active_offer_counts_by_size(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[int, int], dict[str, int], int]:
    active_offer_ids, state_counts, metadata_by_offer_id = _active_offer_state_summary(
        store=store,
        market_id=market_id,
        clock=clock,
        limit=limit,
    )
    normalized_sizes = (
        {int(size) for size in tracked_sizes if int(size) > 0}
        if tracked_sizes is not None
        else {1, 10, 100}
    )
    active_counts_by_size: dict[int, int] = {size: 0 for size in sorted(normalized_sizes)}
    active_unmapped_offer_ids = 0
    for offer_id in active_offer_ids:
        metadata = metadata_by_offer_id.get(offer_id)
        if metadata is not None and _is_stale_pending_visibility_offer(
            offer_id=offer_id,
            metadata=metadata,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
            clock=clock,
        ):
            active_unmapped_offer_ids += 1
            continue
        size = metadata.size if metadata is not None else None
        if size is None and dexie_size_by_offer_id:
            size = dexie_size_by_offer_id.get(offer_id)
        if size in active_counts_by_size:
            active_counts_by_size[size] = int(active_counts_by_size[size]) + 1
        else:
            active_unmapped_offer_ids += 1
    return active_counts_by_size, state_counts, active_unmapped_offer_ids


def _active_offer_counts_by_size_and_side(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
    dexie_size_by_offer_id: dict[str, int] | None = None,
    tracked_sizes: set[int] | None = None,
) -> tuple[dict[str, dict[int, int]], dict[str, int], int]:
    normalized_sizes = (
        {int(size) for size in tracked_sizes if int(size) > 0}
        if tracked_sizes is not None
        else {1, 10, 100}
    )
    counts_by_side: dict[str, dict[int, int]] = {
        "buy": {size: 0 for size in sorted(normalized_sizes)},
        "sell": {size: 0 for size in sorted(normalized_sizes)},
    }
    active_offer_ids, state_counts, metadata_by_offer_id = _active_offer_state_summary(
        store=store,
        market_id=market_id,
        clock=clock,
        limit=limit,
    )
    active_unmapped_offer_ids = 0
    for offer_id in active_offer_ids:
        metadata = metadata_by_offer_id.get(offer_id)
        if metadata is not None and _is_stale_pending_visibility_offer(
            offer_id=offer_id,
            metadata=metadata,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
            clock=clock,
        ):
            active_unmapped_offer_ids += 1
            continue
        size = metadata.size if metadata is not None else None
        side = metadata.side if metadata is not None else None
        if metadata is None or side is None:
            # Do not assume buy/sell direction when metadata is unavailable.
            active_unmapped_offer_ids += 1
            continue
        if size is None and dexie_size_by_offer_id:
            size = dexie_size_by_offer_id.get(offer_id)
        normalized_side = _normalize_offer_side(side)
        if size in counts_by_side[normalized_side]:
            counts_by_side[normalized_side][size] = int(counts_by_side[normalized_side][size]) + 1
        else:
            active_unmapped_offer_ids += 1
    return counts_by_side, state_counts, active_unmapped_offer_ids


def _inject_reseed_action_if_no_active_offers(
    *,
    strategy_actions: list[PlannedAction],
    strategy_config: StrategyConfig,
    market,
    store: SqliteStore,
    xch_price_usd: float | None,
    clock: datetime,
    dexie_size_by_offer_id: dict[str, int] | None = None,
) -> list[PlannedAction]:
    if strategy_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="strategy_actions_present",
            action_count=len(strategy_actions),
        )
        return strategy_actions
    target_by_size = _strategy_target_counts_by_size(strategy_config)
    active_counts_by_size, state_counts, active_unmapped_offer_ids = _active_offer_counts_by_size(
        store=store,
        market_id=market.market_id,
        clock=clock,
        dexie_size_by_offer_id=dexie_size_by_offer_id,
        tracked_sizes=set(target_by_size.keys()),
    )
    missing_by_size = {
        size: max(0, int(target_by_size.get(size, 0)) - int(active_counts_by_size.get(size, 0)))
        for size in target_by_size
    }
    if sum(missing_by_size.values()) <= 0:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="active_offer_targets_satisfied",
            active_states=sorted(_ACTIVE_OFFER_STATES_FOR_RESEED),
            recent_mempool_window_seconds=_RESEED_MEMPOOL_MAX_AGE_SECONDS,
            state_counts=state_counts,
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            active_unmapped_offer_ids=active_unmapped_offer_ids,
        )
        return strategy_actions

    seed_candidates = evaluate_market(
        state=_strategy_state_from_bucket_counts({}, xch_price_usd=xch_price_usd),
        config=strategy_config,
        clock=clock,
    )
    if not seed_candidates:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="no_seed_candidates",
            pair=strategy_config.pair,
            xch_price_usd=xch_price_usd,
        )
        return strategy_actions

    # Reseed one action per ladder size so the market rehydrates as 1/10/100,
    # not only the smallest denomination.
    one_per_size: dict[int, PlannedAction] = {}
    for candidate in seed_candidates:
        size = int(candidate.size)
        if size not in one_per_size:
            one_per_size[size] = candidate
    reseed_actions: list[PlannedAction] = []
    for size in sorted(one_per_size):
        missing = int(missing_by_size.get(size, 0))
        if missing <= 0:
            continue
        action = one_per_size[size]
        reseed_actions.append(
            PlannedAction(
                size=int(action.size),
                repeat=int(missing),
                pair=action.pair,
                expiry_unit=action.expiry_unit,
                expiry_value=int(action.expiry_value),
                cancel_after_create=action.cancel_after_create,
                reason="offer_size_gap_reseed",
                target_spread_bps=action.target_spread_bps,
            )
        )
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="missing_sizes_no_seed_template",
            missing_by_size=missing_by_size,
            candidate_sizes=sorted(one_per_size),
        )
        return strategy_actions
    reseed_actions, cadence_limited_sizes = _apply_action_cadence_gate(
        actions=reseed_actions,
        target_counts_by_side={"buy": {}, "sell": dict(target_by_size)},
        active_counts_by_side={
            "buy": {},
            "sell": {int(size): int(count) for size, count in active_counts_by_size.items()},
        },
        store=store,
        market_id=market.market_id,
        clock=clock,
    )
    if not reseed_actions:
        _log_market_decision(
            market.market_id,
            "reseed_skip",
            reason="reseed_cadence_gate_active",
            active_counts_by_size=active_counts_by_size,
            target_counts_by_size=target_by_size,
            missing_by_size=missing_by_size,
            cadence_limited_sizes=cadence_limited_sizes,
        )
        return strategy_actions

    _log_market_decision(
        market.market_id,
        "reseed_injected",
        reason="offer_size_gap_reseed",
        sizes=[int(action.size) for action in reseed_actions],
        repeats=[int(action.repeat) for action in reseed_actions],
        action_count=sum(int(action.repeat) for action in reseed_actions),
        active_counts_by_size=active_counts_by_size,
        target_counts_by_size=target_by_size,
        missing_by_size=missing_by_size,
        pair=strategy_config.pair,
        expiry_unit=reseed_actions[0].expiry_unit,
        expiry_value=int(reseed_actions[0].expiry_value),
        cadence_limited_sizes=cadence_limited_sizes,
    )
    return reseed_actions


def _resolve_quote_price_quote_per_base(market) -> float:
    pricing = _market_pricing(market)
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
            "market pricing must define fixed_quote_per_base or min/max_price_quote_per_base"
        )
    return float(quote_price)


def _build_offer_for_action(
    *,
    market,
    action,
    xch_price_usd: float | None,
    network: str,
    keyring_yaml_path: str,
) -> dict[str, Any]:
    from greenfloor.offer_builder import build_offer_text

    side = _normalize_offer_side(getattr(action, "side", "sell"))
    if side == "buy":
        return {
            "status": "skipped",
            "reason": "offer_builder_failed:buy_side_requires_cloud_wallet_path",
            "offer": None,
        }
    pricing = _market_pricing(market)
    try:
        quote_price = _resolve_quote_price_quote_per_base(market)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"offer_builder_failed:{exc}",
            "offer": None,
        }
    resolved_quote_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    payload = {
        "market_id": market.market_id,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": resolved_quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "size_base_units": int(action.size),
        "pair": action.pair,
        "reason": action.reason,
        "side": side,
        "xch_price_usd": xch_price_usd,
        "target_spread_bps": action.target_spread_bps,
        "expiry_unit": action.expiry_unit,
        "expiry_value": int(action.expiry_value),
        "quote_price_quote_per_base": quote_price,
        "base_unit_mojo_multiplier": int(
            pricing.get(
                "base_unit_mojo_multiplier",
                default_mojo_multiplier_for_asset(str(market.base_asset)),
            )
        ),
        "quote_unit_mojo_multiplier": int(
            pricing.get(
                "quote_unit_mojo_multiplier",
                default_mojo_multiplier_for_asset(str(resolved_quote_asset)),
            )
        ),
        "key_id": market.signer_key_id,
        "keyring_yaml_path": keyring_yaml_path,
        "network": network,
        "asset_id": market.base_asset,
    }
    try:
        offer = build_offer_text(payload)
    except Exception as exc:
        return {"status": "skipped", "reason": f"offer_builder_failed:{exc}", "offer": None}
    return {"status": "executed", "reason": "offer_builder_success", "offer": offer}


def _cloud_wallet_configured(program: Any) -> bool:
    required = (
        "cloud_wallet_base_url",
        "cloud_wallet_user_key_id",
        "cloud_wallet_private_key_pem_path",
        "cloud_wallet_vault_id",
    )
    return all(str(getattr(program, key, "")).strip() for key in required)


def _new_cloud_wallet_adapter_for_daemon(program: Any) -> CloudWalletAdapter:
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url=str(program.cloud_wallet_base_url).strip(),
            user_key_id=str(program.cloud_wallet_user_key_id).strip(),
            private_key_pem_path=str(program.cloud_wallet_private_key_pem_path).strip(),
            vault_id=str(program.cloud_wallet_vault_id).strip(),
            network=str(program.app_network).strip(),
            kms_key_id=str(getattr(program, "cloud_wallet_kms_key_id", "")).strip() or None,
            kms_region=str(getattr(program, "cloud_wallet_kms_region", "")).strip() or None,
            kms_public_key_hex=str(getattr(program, "cloud_wallet_kms_public_key_hex", "")).strip()
            or None,
        )
    )


def _cloud_wallet_spendable_amounts_by_asset(
    *,
    wallet: CloudWalletAdapter,
    asset_ids: set[str],
) -> dict[str, int]:
    profiles = _cloud_wallet_spendable_profiles_by_asset(wallet=wallet, asset_ids=asset_ids)
    return {asset_id: int(profile.get("total", 0)) for asset_id, profile in profiles.items()}


def _cloud_wallet_spendable_profiles_by_asset(
    *,
    wallet: CloudWalletAdapter,
    asset_ids: set[str],
    scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> dict[str, dict[str, int]]:
    requested_asset_ids = {str(asset_id).strip() for asset_id in asset_ids if str(asset_id).strip()}
    profiles: dict[str, dict[str, int]] = {
        asset_id: {"total": 0, "max_single": 0, "coin_count": 0, "max_single_known": 0}
        for asset_id in requested_asset_ids
    }
    if not requested_asset_ids:
        return profiles

    def _wallet_asset_amounts_for_scope(
        *, scoped_asset_id: str
    ) -> tuple[int | None, int | None, int | None]:
        if not hasattr(wallet, "_graphql"):
            return None, None, None
        query = """
query walletAssetAmounts($walletId: ID!, $assetId: ID!) {
  wallet(id: $walletId) {
    asset(assetId: $assetId) {
      totalAmount
      spendableAmount
      lockedAmount
    }
  }
}
"""
        try:
            payload = wallet._graphql(  # noqa: SLF001
                query=query,
                variables={"walletId": wallet.vault_id, "assetId": scoped_asset_id},
            )
        except Exception:
            return None, None, None
        wallet_payload = payload.get("wallet") if isinstance(payload, dict) else None
        if not isinstance(wallet_payload, dict):
            return None, None, None
        asset_payload = wallet_payload.get("asset")
        if not isinstance(asset_payload, dict):
            return None, None, None
        try:
            total_amount = int(asset_payload.get("totalAmount", 0))
            spendable_amount = int(asset_payload.get("spendableAmount", 0))
            locked_amount = int(asset_payload.get("lockedAmount", 0))
        except (TypeError, ValueError):
            return None, None, None
        return total_amount, spendable_amount, locked_amount

    # Query each requested asset directly. Some wallet backends can return
    # incomplete/unhelpful results for broad unfiltered inventory reads, while
    # asset-scoped reads remain accurate.
    for requested_asset_id in requested_asset_ids:
        requested_asset_id_lower = requested_asset_id.lower()
        profile = profiles[requested_asset_id]
        try:
            if scoped_list_cache is not None:
                coins = scoped_list_cache.list_coins_scoped(resolved_asset_id=requested_asset_id)
            else:
                coins = wallet.list_coins(asset_id=requested_asset_id)
        except TypeError:
            # Backward-compatible fallback for adapters/test doubles that do
            # not yet accept an `asset_id` keyword.
            coins = wallet.list_coins()
        except Exception as exc:
            _daemon_logger.warning(
                "cloud_wallet_inventory_lookup_failed asset_id=%s error=%s",
                requested_asset_id,
                exc,
            )
            _total_amount, spendable_amount, _locked_amount = _wallet_asset_amounts_for_scope(
                scoped_asset_id=requested_asset_id
            )
            if spendable_amount is not None and spendable_amount > 0:
                profile["total"] = max(int(profile.get("total", 0)), int(spendable_amount))
                _daemon_logger.info(
                    "cloud_wallet_inventory_lookup_fallback asset_id=%s source=wallet_asset spendable=%s",
                    requested_asset_id,
                    spendable_amount,
                )
            continue

        profile["max_single_known"] = 1
        for coin in coins:
            if not isinstance(coin, dict):
                continue
            state = str(coin.get("state", "")).strip().upper()
            if state not in _CLOUD_WALLET_SPENDABLE_STATES:
                continue
            if not _cloud_wallet_coin_matches_asset_scope(
                coin=coin,
                scoped_asset_id=requested_asset_id_lower,
            ):
                continue
            try:
                amount = int(coin.get("amount", 0))
            except (TypeError, ValueError):
                amount = 0
            if amount <= 0:
                continue
            profile["total"] += amount
            profile["coin_count"] += 1
            if amount > int(profile.get("max_single", 0)):
                profile["max_single"] = amount
        if int(profile.get("total", 0)) <= 0:
            _total_amount, spendable_amount, _locked_amount = _wallet_asset_amounts_for_scope(
                scoped_asset_id=requested_asset_id
            )
            if spendable_amount is not None and spendable_amount > 0:
                profile["total"] = int(spendable_amount)
                _daemon_logger.info(
                    "cloud_wallet_inventory_lookup_fallback asset_id=%s source=wallet_asset spendable=%s",
                    requested_asset_id,
                    spendable_amount,
                )
    return profiles


def _base_unit_mojo_multiplier_for_market(*, market: Any) -> int:
    pricing = getattr(market, "pricing", {}) or {}
    default_multiplier = default_mojo_multiplier_for_asset(str(getattr(market, "base_asset", "")))
    try:
        multiplier = int(pricing.get("base_unit_mojo_multiplier", default_multiplier))
    except (TypeError, ValueError):
        multiplier = default_multiplier
    return max(1, multiplier)


def _cloud_wallet_spendable_base_unit_coin_amounts(
    *,
    wallet: CloudWalletAdapter,
    resolved_asset_id: str,
    base_unit_mojo_multiplier: int,
    canonical_asset_id: str,
    scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> list[int]:
    target_asset = str(resolved_asset_id).strip().lower()
    if not target_asset:
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    try:
        if scoped_list_cache is not None:
            coins = scoped_list_cache.list_coins_scoped(resolved_asset_id=resolved_asset_id)
        else:
            coins = wallet.list_coins(asset_id=resolved_asset_id)
    except Exception:
        return []
    amounts_base_units: list[int] = []
    direct_lookup_cache: dict[str, bool] = {}
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        state = str(coin.get("state", "")).strip().upper()
        if state not in _CLOUD_WALLET_SPENDABLE_STATES:
            continue
        if not _cloud_wallet_coin_matches_asset_scope(coin=coin, scoped_asset_id=target_asset):
            continue
        if not _coin_meets_coin_op_min_amount(coin, canonical_asset_id=canonical_asset_id):
            continue
        if not _coin_matches_direct_spendable_lookup(
            wallet=wallet,
            coin=coin,
            scoped_asset_id=target_asset,
            cache=direct_lookup_cache,
        ):
            continue
        try:
            amount_mojos = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            amount_mojos = 0
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts_base_units.append(amount_base_units)
    return amounts_base_units


def _coinset_cat_spendable_base_unit_coin_amounts(
    *,
    canonical_asset_id: str,
    receive_address: str,
    network: str,
    base_unit_mojo_multiplier: int,
) -> list[int]:
    asset_hex = str(canonical_asset_id).strip().lower()
    if not asset_hex or not is_hex_id(asset_hex):
        return []
    try:
        import chia_wallet_sdk as sdk  # type: ignore[import-untyped]

        address = sdk.Address.decode(str(receive_address))
        inner_puzzle_hash = bytes(address.puzzle_hash)
        asset_id_bytes = bytes.fromhex(asset_hex)
        cat_puzzle_hash = sdk.cat_puzzle_hash(asset_id_bytes, inner_puzzle_hash)
        coinset = CoinsetAdapter(network=str(network))
        records = coinset.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=f"0x{bytes(cat_puzzle_hash).hex()}",
            include_spent_coins=False,
        )
    except Exception:
        return []
    multiplier = max(1, int(base_unit_mojo_multiplier))
    amounts: list[int] = []
    for record in records:
        if not isinstance(record, dict):
            continue
        coin_payload = record.get("coin")
        if not isinstance(coin_payload, dict):
            continue
        try:
            amount_mojos = int(coin_payload.get("amount", 0))
        except (TypeError, ValueError):
            continue
        if amount_mojos <= 0:
            continue
        amount_base_units = amount_mojos // multiplier
        if amount_base_units > 0:
            amounts.append(amount_base_units)
    return amounts


def _select_spendable_coins_for_target_amount(
    *,
    coins: list[dict[str, Any]],
    target_amount: int,
) -> tuple[list[str], int, bool]:
    """Pick spendable input coins to reach target; prefer exact sum first.

    Returns (coin_ids, selected_total, exact_match).
    """
    required = int(target_amount)
    if required <= 0:
        return [], 0, False
    entries: list[tuple[str, int]] = []
    for coin in coins:
        if not isinstance(coin, dict):
            continue
        coin_id = str(coin.get("id", "")).strip()
        if not coin_id:
            continue
        try:
            amount = int(coin.get("amount", 0))
        except (TypeError, ValueError):
            amount = 0
        if amount <= 0:
            continue
        entries.append((coin_id, amount))
    if not entries:
        return [], 0, False

    max_amount = max(amount for _, amount in entries)
    cap = required + max_amount
    # Guard memory on unusually large amount domains.
    if cap > 500_000:
        ordered = sorted(entries, key=lambda row: row[1], reverse=True)
        picked_ids: list[str] = []
        running = 0
        for coin_id, amount in ordered:
            picked_ids.append(coin_id)
            running += amount
            if running >= required:
                return picked_ids, running, running == required
        return [], 0, False

    best: dict[int, list[int]] = {0: []}
    for idx, (_coin_id, amount) in enumerate(entries):
        snapshot = list(best.items())
        for prev_sum, subset in snapshot:
            next_sum = int(prev_sum) + int(amount)
            if next_sum > cap:
                continue
            candidate = subset + [idx]
            existing = best.get(next_sum)
            if existing is None or len(candidate) < len(existing):
                best[next_sum] = candidate

    exact_subset = best.get(required)
    if exact_subset is not None and len(exact_subset) > 0:
        ids = [entries[i][0] for i in exact_subset]
        total = sum(entries[i][1] for i in exact_subset)
        return ids, total, True

    overs = [s for s in best.keys() if s > required]
    if not overs:
        return [], 0, False
    best_over = min(
        overs,
        key=lambda s: (
            int(s) - required,
            len(best.get(s, [])),
            int(s),
        ),
    )
    subset = best.get(best_over, [])
    if not subset:
        return [], 0, False
    ids = [entries[i][0] for i in subset]
    total = sum(entries[i][1] for i in subset)
    return ids, total, False


def _reservation_request_for_cloud_offer(*, market: Any, action: Any) -> dict[str, int]:
    return _reservation_request_for_cloud_offer_with_assets(
        market=market,
        action=action,
        resolved_base_asset_id=str(
            getattr(market, "cloud_wallet_base_global_id", "") or getattr(market, "base_asset", "")
        ).strip(),
        resolved_quote_asset_id=str(
            getattr(market, "cloud_wallet_quote_global_id", "")
            or getattr(market, "quote_asset", "")
        ).strip(),
        fee_asset_id="xch",
        fee_amount_mojos=0,
    )


def _reservation_request_for_cloud_offer_with_assets(
    *,
    market: Any,
    action: Any,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    fee_asset_id: str,
    fee_amount_mojos: int,
) -> dict[str, int]:
    pricing = market.pricing or {}
    base_multiplier = int(pricing.get("base_unit_mojo_multiplier", 1000))
    quote_multiplier = int(pricing.get("quote_unit_mojo_multiplier", 1000))
    base_asset_id = str(resolved_base_asset_id or "").strip()
    quote_asset_id = str(resolved_quote_asset_id or "").strip()
    if not base_asset_id or not quote_asset_id:
        return {}
    side = _normalize_offer_side(getattr(action, "side", "sell"))
    base_amount = int(action.size) * base_multiplier
    quote_amount = int(
        round(
            float(action.size)
            * float(_resolve_quote_price_quote_per_base(market))
            * float(quote_multiplier)
        )
    )
    offer_asset_id = quote_asset_id if side == "buy" else base_asset_id
    offer_amount = quote_amount if side == "buy" else base_amount
    if offer_amount <= 0:
        return {}
    request: dict[str, int] = {offer_asset_id: offer_amount}
    fee_asset = str(fee_asset_id or "").strip()
    if fee_asset and int(fee_amount_mojos) > 0:
        request[fee_asset] = int(request.get(fee_asset, 0)) + int(fee_amount_mojos)
    return request


def _estimate_cloud_offer_fee_reservation_mojos(*, program: Any) -> int:
    _ = program
    # Offer files must always be created with zero fees. Fees are only used
    # for coin split/combine operations outside offer creation.
    return 0


def _resolve_cloud_wallet_offer_asset_ids_for_reservation(
    *,
    program: Any,
    market: Any,
    wallet: CloudWalletAdapter,
) -> tuple[str, str, str]:
    quote_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(getattr(market, "quote_asset", "")),
        network=str(getattr(program, "app_network", "mainnet")),
    )
    resolved_base_asset_id, resolved_quote_asset_id = resolve_cloud_wallet_offer_asset_ids(
        wallet=wallet,
        base_asset_id=str(getattr(market, "base_asset", "")).strip(),
        quote_asset_id=str(quote_asset).strip(),
        base_symbol_hint=str(getattr(market, "base_symbol", "") or ""),
        quote_symbol_hint=str(getattr(market, "quote_asset", "") or ""),
        base_global_id_hint=str(getattr(market, "cloud_wallet_base_global_id", "") or ""),
        quote_global_id_hint=str(getattr(market, "cloud_wallet_quote_global_id", "") or ""),
        program_home_dir=str(getattr(program, "home_dir", "") or ""),
    )
    resolved_xch_asset_id, _ = resolve_cloud_wallet_offer_asset_ids(
        wallet=wallet,
        base_asset_id="xch",
        quote_asset_id=str(quote_asset).strip(),
        base_symbol_hint="xch",
        quote_symbol_hint=str(getattr(market, "quote_asset", "") or ""),
        base_global_id_hint="",
        quote_global_id_hint=str(getattr(market, "cloud_wallet_quote_global_id", "") or ""),
        program_home_dir=str(getattr(program, "home_dir", "") or ""),
    )
    return resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id


def _cloud_wallet_offer_post_fallback(
    *,
    program: Any,
    market: Any,
    size_base_units: int,
    publish_venue: str,
    runtime_dry_run: bool,
    side: str = "sell",
    build_and_post_fn: Callable[..., tuple[int, dict[str, Any]]] | None = None,
) -> dict[str, Any]:
    if build_and_post_fn is None:
        build_and_post_fn = build_and_post_offer_cloud_wallet

    quote_price = _resolve_quote_price_quote_per_base(market)
    artifact_timeout_seconds = int(program.runtime_cloud_wallet_offer_artifact_timeout_seconds)
    exit_code, payload = build_and_post_fn(
        program=program,
        market=market,
        size_base_units=size_base_units,
        repeat=1,
        publish_venue=publish_venue,
        dexie_base_url=str(program.dexie_api_base),
        splash_base_url=str(program.splash_api_base),
        drop_only=True,
        claim_rewards=False,
        quote_price=quote_price,
        dry_run=runtime_dry_run,
        action_side=side,
        offer_artifact_timeout_seconds=artifact_timeout_seconds,
    )
    results = payload.get("results", [])
    result = (
        results[0].get("result", {})
        if isinstance(results, list) and results and isinstance(results[0], dict)
        else {}
    )
    timing_payload = result.get("timing_ms", {}) if isinstance(result, dict) else {}

    def _opt_int(key: str) -> int | None:
        v = timing_payload.get(key) if isinstance(timing_payload, dict) else None
        return int(v) if v is not None else None

    create_ms = _opt_int("create_total_ms")
    publish_ms = _opt_int("publish_ms")
    total_ms = _opt_int("total_ms")
    create_phase_ms = _opt_int("create_phase_ms")
    artifact_wait_ms = _opt_int("artifact_wait_ms")
    if exit_code != 0:
        error = str(result.get("error", "")).strip() if isinstance(result, dict) else ""
        return {
            "success": False,
            "error": error or f"cloud_wallet_fallback_exit_code:{exit_code}",
            "offer_create_ms": create_ms,
            "offer_publish_ms": publish_ms,
            "offer_total_ms": total_ms,
            "offer_create_phase_ms": create_phase_ms,
            "offer_artifact_wait_ms": artifact_wait_ms,
        }
    if not isinstance(results, list) or not results:
        return {"success": False, "error": "cloud_wallet_fallback_missing_results"}
    result = results[0].get("result", {}) if isinstance(results[0], dict) else {}
    if not isinstance(result, dict):
        result = {}
    success = bool(result.get("success", False)) and int(payload.get("publish_failures", 1)) == 0
    return {
        "success": success,
        "offer_id": str(result.get("id", "")).strip() or None,
        "error": str(result.get("error", "")).strip() if not success else "",
        "offer_create_ms": create_ms,
        "offer_publish_ms": publish_ms,
        "offer_total_ms": total_ms,
        "offer_create_phase_ms": create_phase_ms,
        "offer_artifact_wait_ms": artifact_wait_ms,
    }


def _verify_offer_visible_on_dexie(
    *,
    dexie: DexieAdapter,
    offer_id: str,
    attempts: int = 4,
    delay_seconds: float = 1.5,
) -> tuple[bool, str]:
    clean_offer_id = str(offer_id).strip()
    if not clean_offer_id:
        return False, "missing_offer_id"
    for attempt in range(1, max(1, int(attempts)) + 1):
        try:
            payload = dexie.get_offer(clean_offer_id)
        except Exception as exc:
            if attempt >= attempts:
                return False, f"dexie_get_offer_error:{exc}"
            time.sleep(delay_seconds)
            continue
        offer_payload = payload.get("offer") if isinstance(payload, dict) else None
        if isinstance(offer_payload, dict):
            confirmed_id = str(offer_payload.get("id", "")).strip()
            if confirmed_id == clean_offer_id:
                return True, ""
        if attempt < attempts:
            time.sleep(delay_seconds)
    return False, "dexie_offer_not_visible_after_publish"


def _resolve_coinset_ws_url(*, program, coinset_base_url: str) -> str:
    configured = str(getattr(program, "tx_block_websocket_url", "")).strip()
    if configured:
        return configured
    base_url = coinset_base_url.strip()
    if not base_url:
        if program.app_network.strip().lower() in {"testnet", "testnet11"}:
            return "wss://testnet11.api.coinset.org/ws"
        return "wss://api.coinset.org/ws"
    parsed = urllib.parse.urlparse(base_url)
    scheme = "wss" if parsed.scheme == "https" else "ws"
    host = parsed.netloc or parsed.path
    if not host:
        return "wss://api.coinset.org/ws"
    return f"{scheme}://{host}/ws"


def _build_coinset_adapter(*, program, coinset_base_url: str) -> CoinsetAdapter:
    base_url = coinset_base_url.strip() or None
    try:
        return CoinsetAdapter(base_url, network=program.app_network)
    except TypeError as exc:
        if "network" not in str(exc):
            raise
        return CoinsetAdapter(base_url)


def _run_coinset_signal_capture_once(
    *,
    program,
    coinset_base_url: str,
    store: SqliteStore,
) -> None:
    coinset = _build_coinset_adapter(program=program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=program, coinset_base_url=coinset_base_url)

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        new_count = store.observe_mempool_tx_ids(tx_ids)
        if new_count:
            store.add_audit_event(
                "mempool_observed",
                {"new_tx_ids": new_count, "source": "coinset_websocket"},
            )

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return
        confirmed = store.confirm_tx_ids(tx_ids)
        store.add_audit_event(
            "tx_block_confirmed",
            {
                "tx_ids": tx_ids,
                "confirmed_count": confirmed,
                "source": "coinset_websocket",
            },
        )

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        store.add_audit_event(event_type, payload)

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return
        store.add_audit_event(
            "coin_watch_hit",
            {
                "coin_id_count": len(coin_ids),
                "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                "source": "coinset_websocket",
            },
        )

    capture_coinset_websocket_once(
        ws_url=ws_url,
        reconnect_interval_seconds=program.tx_block_websocket_reconnect_interval_seconds,
        capture_window_seconds=max(1, program.tx_block_fallback_poll_interval_seconds),
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )


def _execute_single_cloud_wallet_action(
    *,
    program: Any,
    market: Any,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> dict[str, Any]:
    """Execute a single strategy action via the cloud wallet path."""
    cloud_wallet_post = _cloud_wallet_offer_post_fallback(
        program=program,
        market=market,
        size_base_units=int(action.size),
        publish_venue=publish_venue,
        runtime_dry_run=runtime_dry_run,
        side=_normalize_offer_side(getattr(action, "side", "sell")),
    )
    timing_fields = {
        "offer_create_ms": cloud_wallet_post.get("offer_create_ms"),
        "offer_publish_ms": cloud_wallet_post.get("offer_publish_ms"),
        "offer_total_ms": cloud_wallet_post.get("offer_total_ms"),
        "offer_create_phase_ms": cloud_wallet_post.get("offer_create_phase_ms"),
        "offer_artifact_wait_ms": cloud_wallet_post.get("offer_artifact_wait_ms"),
    }
    if bool(cloud_wallet_post.get("success", False)):
        cloud_wallet_offer_id = str(cloud_wallet_post.get("offer_id", "")).strip()
        if publish_venue == "dexie" and cloud_wallet_offer_id:
            visible, visibility_error = _verify_offer_visible_on_dexie(
                dexie=dexie,
                offer_id=cloud_wallet_offer_id,
            )
            if not visible:
                # Transient 404 → Dexie propagation lag; mark as pending so the
                # active-count reader keeps the offer in scope until the grace
                # period expires (see _is_stale_pending_visibility_offer).
                if is_transient_dexie_visibility_404_error(visibility_error or ""):
                    return {
                        "size": action.size,
                        "side": _normalize_offer_side(getattr(action, "side", "sell")),
                        "status": "executed",
                        "reason": _PENDING_VISIBILITY_REASON,
                        "offer_id": cloud_wallet_offer_id or None,
                        **timing_fields,
                    }
                return {
                    "size": action.size,
                    "side": _normalize_offer_side(getattr(action, "side", "sell")),
                    "status": "skipped",
                    "reason": (f"cloud_wallet_post_not_visible_on_dexie:{visibility_error}"),
                    "offer_id": cloud_wallet_offer_id or None,
                    **timing_fields,
                }
        return {
            "size": action.size,
            "side": _normalize_offer_side(getattr(action, "side", "sell")),
            "status": "executed",
            "reason": "cloud_wallet_post_success",
            "offer_id": cloud_wallet_offer_id or None,
            **timing_fields,
        }
    return {
        "size": action.size,
        "side": _normalize_offer_side(getattr(action, "side", "sell")),
        "status": "skipped",
        "reason": (
            f"cloud_wallet_post_failed:{str(cloud_wallet_post.get('error', 'unknown')).strip()}"
        ),
        "offer_id": None,
        **timing_fields,
    }


def _execute_cloud_wallet_action_with_retry(
    *,
    program: Any,
    market: Any,
    action: Any,
    publish_venue: str,
    runtime_dry_run: bool,
    dexie: DexieAdapter,
) -> dict[str, Any]:
    """Execute a single cloud-wallet action with transient-error retries.

    Raises on non-transient or exhausted retries so the caller can decide
    how to handle the failure (skip-item in sequential, worker-error in parallel).
    """
    attempts_max, backoff_ms, _ = _post_retry_config()
    last_exc: Exception | None = None
    for attempt_index in range(max(1, int(attempts_max))):
        try:
            return _execute_single_cloud_wallet_action(
                program=program,
                market=market,
                action=action,
                publish_venue=publish_venue,
                runtime_dry_run=runtime_dry_run,
                dexie=dexie,
            )
        except Exception as exc:
            last_exc = exc
            if attempt_index >= (
                max(1, int(attempts_max)) - 1
            ) or not _is_transient_cloud_wallet_upstream_error_text(str(exc)):
                raise
            if backoff_ms > 0:
                sleep_seconds = (backoff_ms * (2**attempt_index)) / 1000.0
                time.sleep(float(sleep_seconds))
    raise RuntimeError(str(last_exc or "cloud_wallet_action_retry_exhausted"))


def _execute_single_local_action(
    *,
    market: Any,
    action: Any,
    xch_price_usd: float | None,
    app_network: str,
    keyring_yaml_path: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
    publish_venue: str,
    store: SqliteStore,
) -> dict[str, Any]:
    """Execute a single strategy action via the local build+sign+post path."""
    action_started = time.monotonic()
    build_started = action_started
    built = _build_offer_for_action(
        market=market,
        action=action,
        xch_price_usd=xch_price_usd,
        network=app_network,
        keyring_yaml_path=keyring_yaml_path,
    )
    build_ms = int((time.monotonic() - build_started) * 1000)
    if built.get("status") != "executed":
        built_reason = str(built.get("reason", "offer_builder_skipped"))
        return {
            "size": action.size,
            "side": _normalize_offer_side(getattr(action, "side", "sell")),
            "status": "skipped",
            "reason": built_reason,
            "offer_id": None,
            "offer_create_ms": build_ms,
            "offer_publish_ms": None,
            "offer_total_ms": int((time.monotonic() - action_started) * 1000),
        }
    _, _, cooldown_seconds = _post_retry_config()
    cooldown_key = f"{publish_venue}:{market.market_id}"
    remaining_ms = _cooldown_remaining_ms(_POST_COOLDOWN_UNTIL, cooldown_key)
    if remaining_ms > 0:
        return {
            "size": action.size,
            "side": _normalize_offer_side(getattr(action, "side", "sell")),
            "status": "skipped",
            "reason": f"post_cooldown_active:{remaining_ms}ms",
            "offer_id": None,
            "offer_create_ms": build_ms,
            "offer_publish_ms": None,
            "offer_total_ms": int((time.monotonic() - action_started) * 1000),
        }
    offer_text = str(built["offer"])
    publish_started = time.monotonic()
    post_result, attempt_count, post_error = _post_offer_with_retry(
        publish_venue=publish_venue,
        offer_text=offer_text,
        dexie=dexie,
        splash=splash,
    )
    publish_ms = int((time.monotonic() - publish_started) * 1000)
    success = bool(post_result.get("success", False))
    offer_id_raw = post_result.get("id")
    offer_id = str(offer_id_raw).strip() if offer_id_raw is not None else ""
    if success and offer_id:
        store.upsert_offer_state(
            offer_id=offer_id,
            market_id=market.market_id,
            state=OfferLifecycleState.OPEN.value,
            last_seen_status=0,
        )
        return {
            "size": action.size,
            "side": _normalize_offer_side(getattr(action, "side", "sell")),
            "status": "executed",
            "reason": f"{publish_venue}_post_success",
            "offer_id": offer_id,
            "attempts": attempt_count,
            "offer_create_ms": build_ms,
            "offer_publish_ms": publish_ms,
            "offer_total_ms": int((time.monotonic() - action_started) * 1000),
        }
    _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
    return {
        "size": action.size,
        "side": _normalize_offer_side(getattr(action, "side", "sell")),
        "status": "skipped",
        "reason": f"{publish_venue}_post_retry_exhausted:{post_error}",
        "offer_id": offer_id or None,
        "attempts": attempt_count,
        "offer_create_ms": build_ms,
        "offer_publish_ms": publish_ms,
        "offer_total_ms": int((time.monotonic() - action_started) * 1000),
    }


def _expand_strategy_actions(strategy_actions: list[Any]) -> list[Any]:
    expanded_actions: list[Any] = []
    for action in strategy_actions:
        expanded_actions.extend(action for _ in range(int(action.repeat)))
    return expanded_actions


def _cloud_wallet_skip_item(*, action: Any, reason: str) -> dict[str, Any]:
    return {
        "size": action.size,
        "side": _normalize_offer_side(getattr(action, "side", "sell")),
        "status": "skipped",
        "reason": reason,
        "offer_id": None,
    }


def _single_input_preferred_skip_reason(
    *,
    requested_amounts: dict[str, int],
    spendable_profiles: dict[str, dict[str, int]],
) -> str | None:
    # Prefer single-input offers on our side: if aggregate balance is
    # sufficient but no single spendable coin can satisfy the offered
    # amount, defer posting and let coin-ops combine first.
    primary_request_candidates = [
        (asset_id, int(amount))
        for asset_id, amount in requested_amounts.items()
        if str(asset_id).strip() and int(amount) > 0
    ]
    if not primary_request_candidates:
        return None
    primary_asset_id, primary_needed = max(
        primary_request_candidates, key=lambda pair: int(pair[1])
    )
    primary_profile = spendable_profiles.get(str(primary_asset_id), {})
    primary_total = int(primary_profile.get("total", 0))
    primary_max = int(primary_profile.get("max_single", 0))
    primary_max_known = bool(int(primary_profile.get("max_single_known", 0)))
    if not primary_max_known:
        return None
    if primary_total >= primary_needed and primary_max < primary_needed:
        return (
            "single_input_preferred_requires_combine"
            f":asset_id={primary_asset_id}"
            f":needed={primary_needed}"
            f":max_single={primary_max}"
            f":available={primary_total}"
        )
    return None


def _prepare_parallel_cloud_wallet_submission(
    *,
    market: Any,
    action: Any,
    cloud_wallet: CloudWalletAdapter,
    resolved_base_asset_id: str,
    resolved_quote_asset_id: str,
    resolved_xch_asset_id: str,
    fee_amount_mojos: int,
    scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> tuple[dict[str, int] | None, dict[str, int] | None, dict[str, Any] | None]:
    requested_amounts = _reservation_request_for_cloud_offer_with_assets(
        market=market,
        action=action,
        resolved_base_asset_id=resolved_base_asset_id,
        resolved_quote_asset_id=resolved_quote_asset_id,
        fee_asset_id=resolved_xch_asset_id,
        fee_amount_mojos=fee_amount_mojos,
    )
    if not requested_amounts:
        return (
            None,
            None,
            _cloud_wallet_skip_item(action=action, reason="reservation_invalid_request"),
        )
    spendable_profiles = _cloud_wallet_spendable_profiles_by_asset(
        wallet=cloud_wallet,
        asset_ids=set(requested_amounts.keys()),
        scoped_list_cache=scoped_list_cache,
    )
    available_amounts = {
        asset_id: int(profile.get("total", 0)) for asset_id, profile in spendable_profiles.items()
    }
    single_input_skip_reason = _single_input_preferred_skip_reason(
        requested_amounts=requested_amounts,
        spendable_profiles=spendable_profiles,
    )
    if single_input_skip_reason:
        return None, None, _cloud_wallet_skip_item(action=action, reason=single_input_skip_reason)
    return requested_amounts, available_amounts, None


def _execute_strategy_actions(
    *,
    market,
    strategy_actions: list,
    runtime_dry_run: bool,
    xch_price_usd: float | None,
    dexie: DexieAdapter,
    splash: SplashAdapter | None = None,
    publish_venue: str = "dexie",
    store: SqliteStore,
    app_network: str = "mainnet",
    signer_key_registry: dict[str, Any] | None = None,
    program: Any | None = None,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    cloud_wallet_scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    executed_count = 0
    signer_key_id = str(getattr(market, "signer_key_id", "") or "").strip()
    signer_key = (signer_key_registry or {}).get(signer_key_id)
    if isinstance(signer_key, dict):
        keyring_yaml_path = str(signer_key.get("keyring_yaml_path", "") or "").strip()
    else:
        keyring_yaml_path = str(getattr(signer_key, "keyring_yaml_path", "") or "").strip()
    expanded_actions = _expand_strategy_actions(strategy_actions)
    can_parallelize_cloud_offers = (
        program is not None
        and _cloud_wallet_configured(program)
        and bool(getattr(program, "runtime_offer_parallelism_enabled", False))
        and not runtime_dry_run
        and reservation_coordinator is not None
    )
    if can_parallelize_cloud_offers:
        try:
            assert program is not None
            assert reservation_coordinator is not None
            cloud_wallet = _new_cloud_wallet_adapter_for_daemon(program)
            resolved_base_asset_id, resolved_quote_asset_id, resolved_xch_asset_id = (
                _resolve_cloud_wallet_offer_asset_ids_for_reservation(
                    program=program,
                    market=market,
                    wallet=cloud_wallet,
                )
            )
            fee_amount_mojos = _estimate_cloud_offer_fee_reservation_mojos(program=program)
            # Health-check the coordinator once per batch before dispatching.
            # Using an empty request avoids any lease writes while still
            # surfacing storage/runtime failures early so we can fail over.
            reservation_coordinator.try_acquire(
                market_id=str(market.market_id),
                wallet_id=str(program.cloud_wallet_vault_id).strip(),
                requested_amounts={},
                available_amounts={},
            )
            submissions: list[tuple[int, Any, dict[str, int], dict[str, int]]] = []
            for submit_index, action in enumerate(expanded_actions):
                requested_amounts, available_amounts, skip_item = (
                    _prepare_parallel_cloud_wallet_submission(
                        market=market,
                        action=action,
                        cloud_wallet=cloud_wallet,
                        resolved_base_asset_id=resolved_base_asset_id,
                        resolved_quote_asset_id=resolved_quote_asset_id,
                        resolved_xch_asset_id=resolved_xch_asset_id,
                        fee_amount_mojos=fee_amount_mojos,
                        scoped_list_cache=cloud_wallet_scoped_list_cache,
                    )
                )
                if skip_item is not None:
                    items.append(skip_item)
                    continue
                assert requested_amounts is not None
                assert available_amounts is not None
                submissions.append((submit_index, action, requested_amounts, available_amounts))

            if submissions:
                coordinator = reservation_coordinator
                assert coordinator is not None
                wallet_id = str(program.cloud_wallet_vault_id).strip()
                max_workers = min(
                    len(submissions),
                    max(1, int(getattr(program, "runtime_offer_parallelism_max_workers", 4))),
                )
                _log_market_decision(
                    str(getattr(market, "market_id", "")),
                    "parallel_offer_dispatch",
                    planned_count=len(expanded_actions),
                    queued_count=len(submissions),
                    workers=max_workers,
                )

                def _run_parallel_submission(
                    *,
                    submit_index: int,
                    action: Any,
                    requested_amounts: dict[str, int],
                    available_amounts: dict[str, int],
                    queued_at_monotonic: float,
                ) -> dict[str, Any]:
                    queue_wait_ms = int((time.monotonic() - queued_at_monotonic) * 1000)
                    _log_market_decision(
                        str(getattr(market, "market_id", "")),
                        "parallel_offer_queue_wait",
                        submit_index=submit_index,
                        size=int(getattr(action, "size", 0)),
                        side=_normalize_offer_side(getattr(action, "side", "sell")),
                        queue_wait_ms=queue_wait_ms,
                    )
                    acquire_started = time.monotonic()
                    acquired = coordinator.try_acquire(
                        market_id=str(market.market_id),
                        wallet_id=wallet_id,
                        requested_amounts=requested_amounts,
                        available_amounts=available_amounts,
                    )
                    acquire_ms = int((time.monotonic() - acquire_started) * 1000)
                    if not acquired.ok or not acquired.reservation_id:
                        return {
                            **_cloud_wallet_skip_item(
                                action=action,
                                reason=str(acquired.error or "reservation_rejected"),
                            ),
                            "queue_wait_ms": queue_wait_ms,
                            "reservation_acquire_ms": acquire_ms,
                        }
                    reservation_id = str(acquired.reservation_id)
                    reserved_at = time.monotonic()
                    _log_market_decision(
                        str(getattr(market, "market_id", "")),
                        "parallel_offer_reservation_acquired",
                        submit_index=submit_index,
                        reservation_id=reservation_id,
                        queue_wait_ms=queue_wait_ms,
                        reservation_acquire_ms=acquire_ms,
                    )
                    try:
                        item = _execute_cloud_wallet_action_with_retry(
                            program=program,
                            market=market,
                            action=action,
                            publish_venue=publish_venue,
                            runtime_dry_run=runtime_dry_run,
                            dexie=dexie,
                        )
                    except Exception as exc:
                        item = {
                            "size": 0,
                            "side": "sell",
                            "status": "skipped",
                            "reason": f"parallel_offer_worker_error:{exc}",
                            "offer_id": None,
                        }
                    release_status = (
                        "released_success"
                        if str(item.get("status", "")).strip().lower() == "executed"
                        else "released_failed"
                    )
                    coordinator.release(reservation_id=reservation_id, status=release_status)
                    reservation_hold_ms = int((time.monotonic() - reserved_at) * 1000)
                    _log_market_decision(
                        str(getattr(market, "market_id", "")),
                        "parallel_offer_reservation_released",
                        submit_index=submit_index,
                        reservation_id=reservation_id,
                        release_status=release_status,
                        reservation_hold_ms=reservation_hold_ms,
                    )
                    item["reservation_id"] = reservation_id
                    item["queue_wait_ms"] = queue_wait_ms
                    item["reservation_acquire_ms"] = acquire_ms
                    item["reservation_hold_ms"] = reservation_hold_ms
                    return item

                with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
                    future_to_submission: dict[concurrent.futures.Future[dict[str, Any]], int] = {}
                    for submit_index, action, requested_amounts, available_amounts in submissions:
                        future = pool.submit(
                            _run_parallel_submission,
                            submit_index=submit_index,
                            action=action,
                            requested_amounts=requested_amounts,
                            available_amounts=available_amounts,
                            queued_at_monotonic=time.monotonic(),
                        )
                        future_to_submission[future] = submit_index
                    submitted_items: list[tuple[int, dict[str, Any]]] = []
                    for future in concurrent.futures.as_completed(future_to_submission):
                        submit_index = future_to_submission[future]
                        try:
                            item = future.result()
                        except Exception as exc:
                            item = {
                                "size": 0,
                                "side": "sell",
                                "status": "skipped",
                                "reason": f"parallel_offer_worker_error:{exc}",
                                "offer_id": None,
                            }
                        submitted_items.append((submit_index, item))
                    for _, item in sorted(submitted_items, key=lambda pair: pair[0]):
                        _log_offer_action_timing(str(getattr(market, "market_id", "")), item)
                        if item.get("status") == "executed":
                            executed_count += 1
                        items.append(item)
                _, _, cooldown_seconds = _post_retry_config()
                transient_parallel_failures = sum(
                    1
                    for _submit_idx, item in submitted_items
                    if str(item.get("status", "")).strip().lower() == "skipped"
                    and _is_transient_cloud_wallet_upstream_error_text(str(item.get("reason", "")))
                )
                total_parallel = len(submitted_items)
                if (
                    total_parallel > 0
                    and cooldown_seconds > 0
                    and transient_parallel_failures >= max(2, (total_parallel + 1) // 2)
                ):
                    cooldown_key = f"{publish_venue}:{market.market_id}"
                    _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
                    _log_market_decision(
                        str(getattr(market, "market_id", "")),
                        "parallel_offer_transient_cooldown",
                        transient_failures=transient_parallel_failures,
                        total_parallel=total_parallel,
                        cooldown_seconds=cooldown_seconds,
                    )
            return {
                "planned_count": len(expanded_actions),
                "executed_count": executed_count,
                "items": items,
            }
        except Exception as exc:
            store.add_audit_event(
                "offer_parallel_fallback",
                {
                    "market_id": str(getattr(market, "market_id", "")),
                    "error": str(exc),
                    "reason": "reservation_parallel_path_failed",
                },
                market_id=str(getattr(market, "market_id", "")),
            )
            can_parallelize_cloud_offers = False
    if not can_parallelize_cloud_offers:
        for action in expanded_actions:
            if runtime_dry_run:
                items.append(
                    {
                        "size": action.size,
                        "side": _normalize_offer_side(getattr(action, "side", "sell")),
                        "status": "planned",
                        "reason": "dry_run",
                        "offer_id": None,
                    }
                )
                continue
            if program is not None and _cloud_wallet_configured(program):
                try:
                    item = _execute_cloud_wallet_action_with_retry(
                        program=program,
                        market=market,
                        action=action,
                        publish_venue=publish_venue,
                        runtime_dry_run=runtime_dry_run,
                        dexie=dexie,
                    )
                except Exception as exc:
                    item = {
                        "size": action.size,
                        "side": _normalize_offer_side(getattr(action, "side", "sell")),
                        "status": "skipped",
                        "reason": f"cloud_wallet_action_error:{exc}",
                        "offer_id": None,
                    }
            else:
                item = _execute_single_local_action(
                    market=market,
                    action=action,
                    xch_price_usd=xch_price_usd,
                    app_network=app_network,
                    keyring_yaml_path=keyring_yaml_path,
                    dexie=dexie,
                    splash=splash,
                    publish_venue=publish_venue,
                    store=store,
                )
            if item.get("status") == "executed":
                executed_count += 1
            _log_offer_action_timing(str(getattr(market, "market_id", "")), item)
            items.append(item)
    return {
        "planned_count": len(expanded_actions),
        "executed_count": executed_count,
        "items": items,
    }


def _execute_cancel_policy_for_market(
    *,
    market,
    offers: list[dict[str, Any]],
    runtime_dry_run: bool,
    current_xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    dexie: DexieAdapter,
    store: SqliteStore,
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    move_bps = _abs_move_bps(current_xch_price_usd, previous_xch_price_usd)
    quote_type = str(market.quote_asset_type).strip().lower()
    pricing = _market_pricing(market)
    stable_vs_unstable = bool(pricing.get("cancel_policy_stable_vs_unstable", False))
    threshold_bps = _cancel_move_threshold_bps(market=market)
    if quote_type != "unstable":
        return {
            "eligible": False,
            "triggered": False,
            "reason": "not_unstable_leg_market",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if not stable_vs_unstable:
        return {
            "eligible": False,
            "triggered": False,
            "reason": "not_stable_vs_unstable_market",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if move_bps is None:
        return {
            "eligible": True,
            "triggered": False,
            "reason": "missing_price_baseline",
            "move_bps": None,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }
    if move_bps < float(threshold_bps):
        return {
            "eligible": True,
            "triggered": False,
            "reason": "price_move_below_threshold",
            "move_bps": move_bps,
            "threshold_bps": threshold_bps,
            "planned_count": 0,
            "executed_count": 0,
            "items": items,
        }

    target_offer_ids: list[str] = []
    for offer in offers:
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        status = int(offer.get("status", -1))
        if status == 0:
            target_offer_ids.append(offer_id)

    executed_count = 0
    _, _, cooldown_seconds = _cancel_retry_config()
    cooldown_key = f"cancel:{market.market_id}"
    for offer_id in target_offer_ids:
        if runtime_dry_run:
            items.append({"offer_id": offer_id, "status": "planned", "reason": "dry_run"})
            continue

        remaining_ms = _cooldown_remaining_ms(_CANCEL_COOLDOWN_UNTIL, cooldown_key)
        if remaining_ms > 0:
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "skipped",
                    "reason": f"cancel_cooldown_active:{remaining_ms}ms",
                }
            )
            continue
        result, attempt_count, cancel_error = _cancel_offer_with_retry(
            dexie=dexie,
            offer_id=offer_id,
        )
        success = bool(result.get("success", False))
        if success:
            executed_count += 1
            store.upsert_offer_state(
                offer_id=offer_id,
                market_id=market.market_id,
                state="cancelled",
                last_seen_status=3,
            )
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "executed",
                    "reason": "cancelled_on_strong_unstable_move",
                    "attempts": attempt_count,
                }
            )
        else:
            _set_cooldown(_CANCEL_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
            items.append(
                {
                    "offer_id": offer_id,
                    "status": "skipped",
                    "reason": f"cancel_retry_exhausted:{cancel_error}",
                    "attempts": attempt_count,
                }
            )

    return {
        "eligible": True,
        "triggered": True,
        "reason": "strong_unstable_price_move",
        "move_bps": move_bps,
        "threshold_bps": threshold_bps,
        "planned_count": len(target_offer_ids),
        "executed_count": executed_count,
        "items": items,
    }


@dataclass(slots=True)
class _MarketCycleResult:
    cycle_errors: int = 0
    strategy_planned: int = 0
    strategy_executed: int = 0
    cancel_triggered: bool = False
    cancel_planned: int = 0
    cancel_executed: int = 0
    immediate_requeue_requested: bool = False
    immediate_requeue_signals: list[str] = field(default_factory=list)


@dataclass(slots=True)
class _MarketDispatchState:
    cursor: int = 0
    immediate_requeue_ids: deque[str] = field(default_factory=deque)


def _enqueue_immediate_requeue_market(dispatch_state: _MarketDispatchState, market_id: str) -> None:
    clean_market_id = str(market_id).strip()
    if not clean_market_id:
        return
    deduped_existing = deque(
        mid for mid in dispatch_state.immediate_requeue_ids if mid != clean_market_id
    )
    deduped_existing.appendleft(clean_market_id)
    dispatch_state.immediate_requeue_ids = deduped_existing


def _select_market_batch(
    *,
    enabled_markets: list[Any],
    slot_count: int,
    dispatch_state: _MarketDispatchState,
) -> tuple[list[Any], list[str]]:
    enabled_by_id: dict[str, Any] = {
        str(getattr(market, "market_id", "")).strip(): market for market in enabled_markets
    }
    enabled_ids = [market_id for market_id in enabled_by_id if market_id]
    if not enabled_ids:
        dispatch_state.immediate_requeue_ids = deque()
        dispatch_state.cursor = 0
        return [], []

    max_slots = max(1, int(slot_count))
    if max_slots >= len(enabled_ids):
        # Keep only currently enabled markets in the requeue deque.
        dispatch_state.immediate_requeue_ids = deque(
            mid for mid in dispatch_state.immediate_requeue_ids if mid in enabled_by_id
        )
        return [enabled_by_id[mid] for mid in enabled_ids], []

    selected_ids: list[str] = []
    selected_set: set[str] = set()
    retained_requeues: deque[str] = deque()
    consumed_requeues: list[str] = []
    for market_id in list(dispatch_state.immediate_requeue_ids):
        if market_id not in enabled_by_id:
            continue
        if market_id in selected_set:
            continue
        if len(selected_ids) < max_slots:
            selected_ids.append(market_id)
            selected_set.add(market_id)
            consumed_requeues.append(market_id)
        else:
            retained_requeues.append(market_id)
    dispatch_state.immediate_requeue_ids = retained_requeues

    round_robin_slots = max_slots - len(selected_ids)
    if round_robin_slots > 0:
        total_enabled = len(enabled_ids)
        start_idx = dispatch_state.cursor % total_enabled
        last_rr_idx: int | None = None
        for step in range(total_enabled):
            idx = (start_idx + step) % total_enabled
            market_id = enabled_ids[idx]
            if market_id in selected_set:
                continue
            selected_ids.append(market_id)
            selected_set.add(market_id)
            last_rr_idx = idx
            if len(selected_ids) >= max_slots:
                break
        if last_rr_idx is not None:
            dispatch_state.cursor = (last_rr_idx + 1) % total_enabled

    selected_markets = [
        enabled_by_id[market_id] for market_id in selected_ids if market_id in enabled_by_id
    ]
    return selected_markets, consumed_requeues


def _detect_stale_open_offers_for_requeue(
    *,
    store: SqliteStore,
    dexie: DexieAdapter,
    enabled_market_ids: set[str],
    per_market_limit: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFERS_PER_MARKET,
    max_offer_checks: int = _GLOBAL_STALE_OPEN_SWEEP_MAX_OFFER_CHECKS,
) -> dict[str, Any]:
    if not enabled_market_ids:
        return {
            "checked_offer_count": 0,
            "requeue_market_ids": [],
            "hits": [],
        }
    rows = store.list_offer_states(limit=5000)
    tracked_states = {
        OfferLifecycleState.OPEN.value,
        OfferLifecycleState.REFRESH_DUE.value,
    }
    offer_ids_by_market: dict[str, list[str]] = {}
    for row in rows:
        market_id = str(row.get("market_id", "")).strip()
        if market_id not in enabled_market_ids:
            continue
        state = str(row.get("state", "")).strip().lower()
        if state not in tracked_states:
            continue
        offer_id = str(row.get("offer_id", "")).strip()
        if not offer_id:
            continue
        market_offer_ids = offer_ids_by_market.setdefault(market_id, [])
        if offer_id in market_offer_ids:
            continue
        if len(market_offer_ids) >= max(1, int(per_market_limit)):
            continue
        market_offer_ids.append(offer_id)

    checked_offer_count = 0
    requeue_market_ids: set[str] = set()
    hits: list[dict[str, str]] = []
    for market_id, offer_ids in offer_ids_by_market.items():
        for offer_id in offer_ids:
            if checked_offer_count >= max(1, int(max_offer_checks)):
                return {
                    "checked_offer_count": checked_offer_count,
                    "requeue_market_ids": sorted(requeue_market_ids),
                    "hits": hits,
                    "truncated": True,
                }
            checked_offer_count += 1
            try:
                payload = dexie.get_offer(offer_id, timeout=5)
                offer = payload.get("offer") if isinstance(payload, dict) else None
                if not isinstance(offer, dict):
                    continue
                status = int(offer.get("status", -1))
                if status in {4, 6}:
                    reason = "tx_confirmed" if status == 4 else "offer_expired"
                    requeue_market_ids.add(market_id)
                    hits.append(
                        {
                            "market_id": market_id,
                            "offer_id": offer_id,
                            "reason": reason,
                        }
                    )
            except Exception as exc:  # pragma: no cover - network dependent
                if _is_dexie_offer_missing_error(exc):
                    requeue_market_ids.add(market_id)
                    hits.append(
                        {
                            "market_id": market_id,
                            "offer_id": offer_id,
                            "reason": "offer_missing_404",
                        }
                    )
                continue

    return {
        "checked_offer_count": checked_offer_count,
        "requeue_market_ids": sorted(requeue_market_ids),
        "hits": hits,
        "truncated": False,
    }


def _reconcile_offer_states(
    *,
    market: Any,
    network: str,
    dexie: DexieAdapter,
    store: SqliteStore,
    now: datetime,
    result: _MarketCycleResult,
) -> tuple[list[dict[str, Any]], dict[str, int], str | None, list[dict[str, Any]]]:
    """Fetch Dexie offers, augment beyond-cap offers, and transition lifecycle states.

    Returns (augmented_offers, dexie_size_by_offer_id, dexie_fetch_error, offers).
    offers is the raw Dexie list (used by cancel policy); augmented_offers includes
    beyond-cap individually-fetched offers.
    """
    dexie_fetch_error: str | None = None
    dexie_offered_asset = resolve_trade_asset_for_dexie(
        asset=str(market.base_asset),
        network=network,
    )
    dexie_requested_asset = _resolve_quote_asset_for_offer(
        quote_asset=str(market.quote_asset),
        network=network,
    )
    try:
        offers = dexie.get_offers(dexie_offered_asset, dexie_requested_asset)
        _log_market_decision(
            market.market_id,
            "dexie_offers_fetched",
            offered=dexie_offered_asset,
            requested=dexie_requested_asset,
            count=len(offers),
        )
    except Exception as exc:  # pragma: no cover - network dependent
        dexie_fetch_error = str(exc)
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "dexie_offers_error",
            error=str(exc),
        )
        store.add_audit_event(
            "dexie_offers_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
        offers = []
    our_offer_ids = _watchlist_offer_ids_from_store(
        store=store,
        market_id=market.market_id,
        clock=now,
    )
    # For any of our active offers not returned by the Dexie list (either genuinely
    # beyond the 20-offer cap, or expired/completed), fetch them individually and
    # add to the offers list so the state-transition loop below can handle expirations.
    # A 5-second timeout prevents a hung TCP connection from stalling the daemon.
    dexie_offer_ids_in_list = {str(o.get("id", "")).strip() for o in offers if o.get("id")}
    beyond_cap_ids = our_offer_ids - dexie_offer_ids_in_list
    augmented_offers = list(offers)
    augmented_by_id: dict[str, dict[str, Any]] = {}
    for offer in augmented_offers:
        if not isinstance(offer, dict):
            continue
        offer_id = str(offer.get("id", "")).strip()
        if not offer_id:
            continue
        augmented_by_id[offer_id] = offer

    # Refresh all of our watched offers individually. Dexie list snapshots can
    # lag status transitions; direct offer fetches make lifecycle state updates
    # deterministic for strategy planning.
    missing_watched_offer_ids: set[str] = set()
    for watched_offer_id in sorted(our_offer_ids):
        try:
            single_payload = dexie.get_offer(watched_offer_id, timeout=5)
            single_offer = single_payload.get("offer") if isinstance(single_payload, dict) else None
            if isinstance(single_offer, dict):
                augmented_by_id[watched_offer_id] = single_offer
        except Exception as exc:  # pragma: no cover - network dependent
            if _is_dexie_offer_missing_error(exc):
                transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRED)
                result.immediate_requeue_requested = True
                result.immediate_requeue_signals.append(OfferSignal.EXPIRED.value)
                missing_watched_offer_ids.add(watched_offer_id)
                _log_market_decision(
                    market.market_id,
                    "offer_transition",
                    offer_id=watched_offer_id,
                    dexie_status=None,
                    signal_source="dexie_get_offer_404",
                    old_state=transition.old_state.value,
                    new_state=transition.new_state.value,
                    signal=transition.signal.value,
                )
                store.upsert_offer_state(
                    offer_id=watched_offer_id,
                    market_id=market.market_id,
                    state=transition.new_state.value,
                    last_seen_status=None,
                )
                store.add_audit_event(
                    "offer_lifecycle_transition",
                    {
                        "offer_id": watched_offer_id,
                        "market_id": market.market_id,
                        "old_state": transition.old_state.value,
                        "new_state": transition.new_state.value,
                        "signal": transition.signal.value,
                        "action": transition.action,
                        "reason": transition.reason,
                        "dexie_status": None,
                        "signal_source": "dexie_get_offer_404",
                        "dexie_error": str(exc),
                        "coinset_tx_ids": [],
                        "coinset_confirmed_tx_ids": [],
                        "coinset_mempool_tx_ids": [],
                    },
                    market_id=market.market_id,
                )
            continue

    for beyond_offer_id in beyond_cap_ids - missing_watched_offer_ids:
        try:
            single_payload = dexie.get_offer(beyond_offer_id, timeout=5)
            single_offer = single_payload.get("offer") if isinstance(single_payload, dict) else None
            if isinstance(single_offer, dict):
                augmented_by_id[beyond_offer_id] = single_offer
        except Exception:  # pragma: no cover - network dependent
            pass
    augmented_offers = list(augmented_by_id.values())
    dexie_size_by_offer_id: dict[str, int] = _build_dexie_size_by_offer_id(
        augmented_offers, str(market.base_asset)
    )
    if dexie_fetch_error is None:
        _update_market_coin_watchlist_from_dexie(
            market=market,
            offers=augmented_offers,
            store=store,
            clock=now,
        )
    for offer in augmented_offers:
        offer_id = str(offer.get("id", ""))
        if not offer_id:
            continue
        if offer_id not in our_offer_ids:
            continue
        status = int(offer.get("status", -1))
        coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(offer)
        signal_source = "dexie_status_fallback"
        coinset_confirmed_tx_ids: list[str] = []
        coinset_mempool_tx_ids: list[str] = []
        if coinset_tx_ids:
            tx_signal_state = store.get_tx_signal_state(coinset_tx_ids)
            for tx_id in coinset_tx_ids:
                signal = tx_signal_state.get(tx_id, {})
                if signal.get("tx_block_confirmed_at"):
                    coinset_confirmed_tx_ids.append(tx_id)
                    continue
                if signal.get("mempool_observed_at"):
                    coinset_mempool_tx_ids.append(tx_id)
        if coinset_confirmed_tx_ids and status != 3:
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.TX_CONFIRMED)
            signal_source = "coinset_webhook"
        elif coinset_mempool_tx_ids:
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.MEMPOOL_SEEN)
            signal_source = "coinset_mempool"
        elif status == 4:
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.TX_CONFIRMED)
        elif status == 6:
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRED)
        elif status == 0:
            # Dexie status 0 means the offer is still listed/open.
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.REFRESH_POSTED)
        else:
            # Non-terminal Dexie fallback statuses are not mempool evidence.
            # Only Coinset mempool signals should drive MEMPOOL_SEEN transitions.
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.REFRESH_POSTED)
        _log_market_decision(
            market.market_id,
            "offer_transition",
            offer_id=offer_id,
            dexie_status=status,
            signal_source=signal_source,
            old_state=transition.old_state.value,
            new_state=transition.new_state.value,
            signal=transition.signal.value,
        )
        store.upsert_offer_state(
            offer_id=offer_id,
            market_id=market.market_id,
            state=transition.new_state.value,
            last_seen_status=status,
        )
        store.add_audit_event(
            "offer_lifecycle_transition",
            {
                "offer_id": offer_id,
                "market_id": market.market_id,
                "old_state": transition.old_state.value,
                "new_state": transition.new_state.value,
                "signal": transition.signal.value,
                "action": transition.action,
                "reason": transition.reason,
                "dexie_status": status,
                "signal_source": signal_source,
                "coinset_tx_ids": coinset_tx_ids,
                "coinset_confirmed_tx_ids": coinset_confirmed_tx_ids,
                "coinset_mempool_tx_ids": coinset_mempool_tx_ids,
            },
            market_id=market.market_id,
        )
        if transition.signal in {OfferSignal.EXPIRED, OfferSignal.TX_CONFIRMED}:
            result.immediate_requeue_requested = True
            result.immediate_requeue_signals.append(transition.signal.value)
    return augmented_offers, dexie_size_by_offer_id, dexie_fetch_error, offers


def _evaluate_and_execute_strategy(
    *,
    market: Any,
    program: Any,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    now: datetime,
    dexie_size_by_offer_id: dict[str, int],
    result: _MarketCycleResult,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    cloud_wallet_scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> tuple[dict[str, dict[int, int]], dict[int, int]]:
    """Evaluate market strategy, inject reseed if needed, and execute offer actions."""
    market_mode = str(getattr(market, "mode", "")).strip().lower()
    strategy_config = _strategy_config_from_market(market)
    tracked_sizes = {
        int(entry.size_base_units)
        for side_entries in (getattr(market, "ladders", {}) or {}).values()
        for entry in side_entries
        if int(getattr(entry, "size_base_units", 0)) > 0
    }
    if not tracked_sizes:
        tracked_sizes = set(_strategy_target_counts_by_size(strategy_config).keys())
    if market_mode == "two_sided":
        offer_counts_by_side, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size_and_side(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        active_offer_counts_by_size = {
            size: int(offer_counts_by_side["buy"].get(size, 0))
            + int(offer_counts_by_side["sell"].get(size, 0))
            for size in sorted(tracked_sizes)
        }
        target_counts_by_side = {
            "buy": _strategy_target_counts_by_size(
                _strategy_config_for_side(market=market, side="buy")
            ),
            "sell": _strategy_target_counts_by_size(
                _strategy_config_for_side(market=market, side="sell")
            ),
        }
    else:
        active_offer_counts_by_size, offer_state_counts, active_unmapped_offer_ids = (
            _active_offer_counts_by_size(
                store=store,
                market_id=market.market_id,
                clock=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                tracked_sizes=tracked_sizes,
            )
        )
        offer_counts_by_side = {
            "buy": {size: 0 for size in sorted(tracked_sizes)},
            "sell": dict(active_offer_counts_by_size),
        }
        target_counts_by_side = {
            "buy": {},
            "sell": _strategy_target_counts_by_size(strategy_config),
        }
    _log_market_decision(
        market.market_id,
        "strategy_state_source",
        source="dexie_offer_coverage",
        active_offer_counts_by_size=active_offer_counts_by_size,
        active_offer_counts_by_side=offer_counts_by_side,
        state_counts=offer_state_counts,
        active_unmapped_offer_ids=active_unmapped_offer_ids,
    )
    if market_mode == "two_sided":
        strategy_actions = _evaluate_two_sided_market_actions(
            market=market,
            counts_by_side=offer_counts_by_side,
            xch_price_usd=xch_price_usd,
            now=now,
        )
    else:
        strategy_actions = evaluate_market(
            state=_strategy_state_from_bucket_counts(
                active_offer_counts_by_size, xch_price_usd=xch_price_usd
            ),
            config=strategy_config,
            clock=now,
        )
    strategy_actions, cadence_limited_sizes = _apply_action_cadence_gate(
        actions=strategy_actions,
        target_counts_by_side=target_counts_by_side,
        active_counts_by_side=offer_counts_by_side,
        store=store,
        market_id=market.market_id,
        clock=now,
    )
    _log_market_decision(
        market.market_id,
        "strategy_evaluated",
        pair=strategy_config.pair,
        mode=market_mode or "sell_only",
        offer_counts=active_offer_counts_by_size,
        xch_price_usd=xch_price_usd,
        action_count=len(strategy_actions),
        cadence_limited_sizes=cadence_limited_sizes,
    )
    if market_mode != "two_sided":
        strategy_actions = _inject_reseed_action_if_no_active_offers(
            strategy_actions=strategy_actions,
            strategy_config=strategy_config,
            market=market,
            store=store,
            xch_price_usd=xch_price_usd,
            clock=now,
            dexie_size_by_offer_id=dexie_size_by_offer_id,
        )
    _log_market_decision(
        market.market_id,
        "strategy_after_reseed",
        action_count=len(strategy_actions),
        reseed_injected=any(
            str(action.reason) == "no_active_offer_reseed" for action in strategy_actions
        ),
    )
    store.add_audit_event(
        "strategy_actions_planned",
        {
            "market_id": market.market_id,
            "xch_price_usd": xch_price_usd,
            "actions": [
                {
                    "size": action.size,
                    "repeat": action.repeat,
                    "pair": action.pair,
                    "expiry_unit": action.expiry_unit,
                    "expiry_value": action.expiry_value,
                    "cancel_after_create": action.cancel_after_create,
                    "reason": action.reason,
                    "target_spread_bps": action.target_spread_bps,
                    "side": _normalize_offer_side(getattr(action, "side", "sell")),
                }
                for action in strategy_actions
            ],
        },
        market_id=market.market_id,
    )
    offer_execution = _execute_strategy_actions(
        market=market,
        strategy_actions=strategy_actions,
        runtime_dry_run=program.runtime_dry_run,
        xch_price_usd=xch_price_usd,
        dexie=dexie,
        splash=splash,
        publish_venue=program.offer_publish_venue,
        store=store,
        app_network=program.app_network,
        signer_key_registry=program.signer_key_registry,
        program=program,
        reservation_coordinator=reservation_coordinator,
        cloud_wallet_scoped_list_cache=cloud_wallet_scoped_list_cache,
    )
    result.strategy_planned += int(offer_execution["planned_count"])
    result.strategy_executed += int(offer_execution["executed_count"])
    _log_market_decision(
        market.market_id,
        "strategy_executed",
        planned_count=offer_execution["planned_count"],
        executed_count=offer_execution["executed_count"],
    )
    store.add_audit_event(
        "strategy_offer_execution",
        {
            "market_id": market.market_id,
            "planned_count": offer_execution["planned_count"],
            "executed_count": offer_execution["executed_count"],
            "items": offer_execution["items"],
        },
        market_id=market.market_id,
    )
    health_payload = _cloud_wallet_market_health_payload(
        market_id=str(market.market_id),
        current_items=list(offer_execution["items"]),
        now=now,
    )
    store.add_audit_event(
        "cloud_wallet_market_health",
        health_payload,
        market_id=market.market_id,
    )
    return offer_counts_by_side, _executed_sell_offer_counts_by_size(offer_execution)


def _plan_and_execute_coin_ops(
    *,
    market: Any,
    program: Any,
    wallet: WalletAdapter,
    store: SqliteStore,
    sell_ladder: list[Any],
    wallet_bucket_counts: dict[int, int],
    active_sell_offer_counts_by_size: dict[int, int] | None,
    newly_executed_sell_offer_counts_by_size: dict[int, int] | None,
    signer_selection: Any,
    state_dir: Path,
) -> None:
    """Plan and execute coin split/combine operations for a market."""
    bucket_counts = _effective_sell_bucket_counts_for_coin_ops(
        sell_ladder=sell_ladder,
        wallet_bucket_counts=wallet_bucket_counts,
        active_sell_offer_counts_by_size=active_sell_offer_counts_by_size,
        newly_executed_sell_offer_counts_by_size=newly_executed_sell_offer_counts_by_size,
    )
    base_unit_mojo_multiplier = _base_unit_mojo_multiplier_for_market(market=market)
    canonical_base_asset_id = str(getattr(market, "base_asset", "")).strip()
    invalid_buckets: list[dict[str, int]] = []
    valid_sell_ladder: list[Any] = []
    for entry in sell_ladder:
        size_base_units = int(getattr(entry, "size_base_units", 0))
        if size_base_units <= 0:
            continue
        target_amount_mojos = size_base_units * int(base_unit_mojo_multiplier)
        if _coin_op_target_amount_allowed(
            amount_mojos=target_amount_mojos,
            canonical_asset_id=canonical_base_asset_id,
        ):
            valid_sell_ladder.append(entry)
            continue
        invalid_buckets.append(
            {
                "size_base_units": size_base_units,
                "target_amount_mojos": int(target_amount_mojos),
                "minimum_allowed_mojos": int(
                    _coin_op_min_amount_mojos(canonical_asset_id=canonical_base_asset_id)
                ),
            }
        )
    if invalid_buckets:
        _log_market_decision(
            market.market_id,
            "coin_ops_skip_sub_minimum_target_amount",
            invalid_bucket_count=len(invalid_buckets),
            invalid_buckets=invalid_buckets,
        )
    if not valid_sell_ladder:
        return
    buckets = [
        BucketSpec(
            size_base_units=e.size_base_units,
            target_count=e.target_count,
            split_buffer_count=e.split_buffer_count,
            combine_when_excess_factor=e.combine_when_excess_factor,
            current_count=int(bucket_counts.get(e.size_base_units, 0)),
        )
        for e in valid_sell_ladder
    ]
    plans = plan_coin_ops(
        buckets=buckets,
        max_operations_per_run=program.coin_ops_max_operations_per_run,
        max_fee_budget_mojos=program.coin_ops_max_daily_fee_budget_mojos,
        split_fee_mojos=program.coin_ops_split_fee_mojos,
        combine_fee_mojos=program.coin_ops_combine_fee_mojos,
    )
    if plans:
        _log_market_decision(
            market.market_id,
            "coin_ops_planned",
            plan_count=len(plans),
            split_plan_count=sum(1 for p in plans if str(p.op_type) == "split"),
            combine_plan_count=sum(1 for p in plans if str(p.op_type) == "combine"),
            split_op_count=sum(int(p.op_count) for p in plans if str(p.op_type) == "split"),
            combine_op_count=sum(int(p.op_count) for p in plans if str(p.op_type) == "combine"),
        )
        projected_fee = projected_coin_ops_fee_mojos(
            plans=plans,
            split_fee_mojos=program.coin_ops_split_fee_mojos,
            combine_fee_mojos=program.coin_ops_combine_fee_mojos,
        )
        spent_today = store.get_daily_fee_spent_mojos_utc()
        executable_plans, overflow_plans = partition_plans_by_budget(
            plans=plans,
            split_fee_mojos=program.coin_ops_split_fee_mojos,
            combine_fee_mojos=program.coin_ops_combine_fee_mojos,
            spent_today_mojos=spent_today,
            max_daily_fee_budget_mojos=program.coin_ops_max_daily_fee_budget_mojos,
        )
        if executable_plans:
            execution = _execute_coin_ops_cloud_wallet_kms_only(
                market=market,
                program=program,
                plans=executable_plans,
                wallet=wallet,
                signer_selection=signer_selection,
                state_dir=state_dir,
            )
            _log_market_decision(
                market.market_id,
                "coin_ops_executed",
                plan_count=len(plans),
                executable_count=len(executable_plans),
                overflow_count=len(overflow_plans),
            )
        else:
            execution = {
                "dry_run": program.runtime_dry_run,
                "planned_count": 0,
                "executed_count": 0,
                "status": "skipped_fee_budget",
                "items": [],
            }
            _log_market_decision(
                market.market_id,
                "coin_ops_skipped_fee_budget",
                plan_count=len(plans),
                overflow_count=len(overflow_plans),
            )
        if overflow_plans:
            store.add_audit_event(
                "coin_ops_partial_or_skipped_fee_budget",
                {
                    "market_id": market.market_id,
                    "spent_today_mojos": spent_today,
                    "projected_mojos": projected_fee,
                    "max_daily_fee_budget_mojos": program.coin_ops_max_daily_fee_budget_mojos,
                    "overflow_plans": [
                        {
                            "op_type": p.op_type,
                            "size_base_units": p.size_base_units,
                            "op_count": p.op_count,
                            "reason": p.reason,
                        }
                        for p in overflow_plans
                    ],
                },
                market_id=market.market_id,
            )
            execution_items = execution.get("items", [])
            execution_items.extend(
                [
                    {
                        "op_type": p.op_type,
                        "size_base_units": p.size_base_units,
                        "op_count": p.op_count,
                        "status": "skipped",
                        "reason": "fee_budget_guard",
                        "operation_id": None,
                    }
                    for p in overflow_plans
                ]
            )
            execution["items"] = execution_items
        execution["planned_count"] = len(plans)
        store.add_audit_event(
            "coin_ops_plan",
            {
                "market_id": market.market_id,
                "projected_fee_mojos": projected_fee,
                "spent_today_mojos": spent_today,
                "plans": [
                    {
                        "op_type": p.op_type,
                        "size_base_units": p.size_base_units,
                        "op_count": p.op_count,
                        "reason": p.reason,
                    }
                    for p in plans
                ],
                "execution": execution,
            },
            market_id=market.market_id,
        )
        for item in execution.get("items", []):
            event_type = f"coin_op_{item.get('status', 'unknown')}"
            op_type = str(item.get("op_type"))
            per_op_fee = (
                program.coin_ops_split_fee_mojos
                if op_type == "split"
                else program.coin_ops_combine_fee_mojos
            )
            op_count = int(item.get("op_count", 0))
            fee_mojos = per_op_fee * op_count if item.get("status") == "executed" else 0
            _log_market_decision(
                market.market_id,
                "coin_op_item_result",
                op_type=op_type,
                status=str(item.get("status", "unknown")),
                op_count=op_count,
                size_base_units=item.get("size_base_units"),
                reason=str(item.get("reason", "")),
                operation_id=item.get("operation_id"),
                fee_mojos=fee_mojos,
            )
            store.add_audit_event(
                event_type,
                {
                    "market_id": market.market_id,
                    "op_type": op_type,
                    "size_base_units": item.get("size_base_units"),
                    "op_count": op_count,
                    "reason": item.get("reason"),
                    "operation_id": item.get("operation_id"),
                    "fee_mojos": fee_mojos,
                },
                market_id=market.market_id,
            )
            store.add_coin_op_ledger_entry(
                market_id=market.market_id,
                op_type=op_type,
                op_count=op_count,
                fee_mojos=fee_mojos,
                status=str(item.get("status", "unknown")),
                reason=str(item.get("reason", "")),
                operation_id=(
                    str(item.get("operation_id")) if item.get("operation_id") is not None else None
                ),
            )
    else:
        _log_market_decision(market.market_id, "coin_ops_no_plans")


def _execute_coin_ops_cloud_wallet_kms_only(
    *,
    market: Any,
    program: Any,
    plans: list[CoinOpPlan],
    wallet: WalletAdapter,
    signer_selection: Any,
    state_dir: Path,
) -> dict[str, Any]:
    _ = wallet, state_dir
    if not _cloud_wallet_configured(program):
        return {
            "dry_run": bool(program.runtime_dry_run),
            "planned_count": len(plans),
            "executed_count": 0,
            "status": "skipped",
            "signer_selection": None,
            "items": [
                {
                    "op_type": plan.op_type,
                    "size_base_units": plan.size_base_units,
                    "op_count": plan.op_count,
                    "status": "skipped",
                    "reason": "cloud_wallet_required_for_coin_ops",
                    "operation_id": None,
                }
                for plan in plans
            ],
        }

    if not str(getattr(program, "cloud_wallet_kms_key_id", "")).strip():
        return {
            "dry_run": bool(program.runtime_dry_run),
            "planned_count": len(plans),
            "executed_count": 0,
            "status": "skipped",
            "signer_selection": None,
            "items": [
                {
                    "op_type": plan.op_type,
                    "size_base_units": plan.size_base_units,
                    "op_count": plan.op_count,
                    "status": "skipped",
                    "reason": "cloud_wallet_kms_required_for_coin_ops",
                    "operation_id": None,
                }
                for plan in plans
            ],
        }

    cloud_wallet = _new_cloud_wallet_adapter_for_daemon(program)
    resolved_base_asset_id, _, _ = _resolve_cloud_wallet_offer_asset_ids_for_reservation(
        program=program,
        market=market,
        wallet=cloud_wallet,
    )

    base_unit_mojo_multiplier = _base_unit_mojo_multiplier_for_market(market=market)
    items: list[dict[str, Any]] = []
    executed_count = 0
    combine_input_cap = _combine_input_coin_cap()
    direct_coin_lookup_cache: dict[str, bool] = {}

    def _spendable_asset_scoped_coins(coins: list[dict[str, Any]]) -> list[dict[str, Any]]:
        scoped: list[dict[str, Any]] = []
        target_asset = str(resolved_base_asset_id).strip().lower()
        canonical_asset_id = str(getattr(market, "base_asset", "")).strip()
        for coin in coins:
            if not isinstance(coin, dict):
                continue
            coin_id = str(coin.get("id", "")).strip()
            if not coin_id:
                continue
            state = str(coin.get("state", "")).strip().upper()
            if state not in _CLOUD_WALLET_SPENDABLE_STATES:
                continue
            if not _cloud_wallet_coin_matches_asset_scope(coin=coin, scoped_asset_id=target_asset):
                continue
            if not _coin_meets_coin_op_min_amount(coin, canonical_asset_id=canonical_asset_id):
                continue
            if not _coin_matches_direct_spendable_lookup(
                wallet=cloud_wallet,
                coin=coin,
                scoped_asset_id=target_asset,
                cache=direct_coin_lookup_cache,
            ):
                continue
            scoped.append(coin)
        return scoped

    for plan in plans:
        op_type = str(plan.op_type)
        op_count = int(plan.op_count)
        size_base_units = int(plan.size_base_units)
        if op_count <= 0 or size_base_units <= 0:
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": "invalid_plan",
                    "operation_id": None,
                }
            )
            continue
        if bool(program.runtime_dry_run):
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "planned",
                    "reason": "dry_run:cloud_wallet_kms",
                    "operation_id": None,
                }
            )
            continue

        try:
            if op_type == "split":
                if op_count == 1:
                    # A one-output split only manufactures bookkeeping churn.
                    # Let the market continue rather than creating a cosmetic
                    # "split 1 coin into 1 coin" transaction.
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "split_single_coin_noop_skipped",
                            "operation_id": None,
                        }
                    )
                    continue
                amount_per_coin_mojos = size_base_units * base_unit_mojo_multiplier
                canonical_asset_id = str(getattr(market, "base_asset", "")).strip()
                # Defensive inner check: _plan_and_execute_coin_ops filters the
                # ladder before reaching here, but callers that bypass that
                # layer (e.g. direct tests or future call sites) also get a
                # clean rejection rather than a failed RPC call.
                if not _coin_op_target_amount_allowed(
                    amount_mojos=amount_per_coin_mojos,
                    canonical_asset_id=canonical_asset_id,
                ):
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "split_amount_below_coin_op_minimum",
                            "operation_id": None,
                            "data": {
                                "amount_per_coin_mojos": int(amount_per_coin_mojos),
                                "minimum_allowed_mojos": int(
                                    _coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                                ),
                            },
                        }
                    )
                    continue
                required_amount = amount_per_coin_mojos * op_count
                coins = cloud_wallet.list_coins(
                    asset_id=resolved_base_asset_id, include_pending=True
                )
                spendable = _spendable_asset_scoped_coins(coins)
                if not spendable:
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "no_spendable_split_coin_available",
                            "operation_id": None,
                        }
                    )
                    continue
                attempted_coin_ids: set[str] = set()
                split_submitted = False
                for attempt_index in range(2):
                    # Re-read before each attempt to avoid selecting stale now-locked coins.
                    fresh = cloud_wallet.list_coins(
                        asset_id=resolved_base_asset_id, include_pending=True
                    )
                    candidate_spendable = [
                        coin
                        for coin in _spendable_asset_scoped_coins(fresh)
                        if str(coin.get("id", "")).strip() not in attempted_coin_ids
                    ]
                    fresh_spendable = [
                        coin
                        for coin in candidate_spendable
                        if int(coin.get("amount", 0)) >= required_amount
                    ]
                    if not fresh_spendable:
                        aggregate_amount = sum(
                            int(coin.get("amount", 0)) for coin in candidate_spendable
                        )
                        if attempt_index == 0 and aggregate_amount >= required_amount:
                            combine_coin_ids, combine_total, exact_match = (
                                _select_spendable_coins_for_target_amount(
                                    coins=candidate_spendable,
                                    target_amount=required_amount,
                                )
                            )
                            if len(combine_coin_ids) >= 2:
                                amount_by_coin_id = {
                                    str(coin.get("id", "")).strip(): int(coin.get("amount", 0))
                                    for coin in candidate_spendable
                                }
                                combine_input_coin_ids = list(combine_coin_ids[:combine_input_cap])
                                combine_cap_applied = len(combine_input_coin_ids) < len(
                                    combine_coin_ids
                                )
                                combine_selected_total = sum(
                                    amount_by_coin_id.get(coin_id, 0)
                                    for coin_id in combine_input_coin_ids
                                )
                                combine_exact_match = combine_selected_total == required_amount
                                combine_target_amount = (
                                    required_amount
                                    if combine_selected_total >= required_amount
                                    else combine_selected_total
                                )
                                if combine_cap_applied and combine_selected_total < required_amount:
                                    _daemon_logger.info(
                                        "coin_ops_combine_cap_progress "
                                        "market_id=%s required_amount=%s selected_total=%s "
                                        "selected_before_cap=%s selected_after_cap=%s input_coin_cap=%s "
                                        "note=%s",
                                        str(getattr(market, "market_id", "")).strip() or "unknown",
                                        int(required_amount),
                                        int(combine_selected_total),
                                        int(len(combine_coin_ids)),
                                        int(len(combine_input_coin_ids)),
                                        int(combine_input_cap),
                                        "submitted capped progress combine; next cycle likely needs only 2-coin combine",
                                    )
                                try:
                                    combine_result = _combine_coins_with_retry(
                                        cloud_wallet=cloud_wallet,
                                        combine_kwargs={
                                            "number_of_coins": len(combine_input_coin_ids),
                                            "fee": int(program.coin_ops_combine_fee_mojos),
                                            "asset_id": resolved_base_asset_id,
                                            "largest_first": True,
                                            "input_coin_ids": combine_input_coin_ids,
                                            "target_amount": combine_target_amount,
                                        },
                                    )
                                except Exception as exc:
                                    items.append(
                                        {
                                            "op_type": op_type,
                                            "size_base_units": size_base_units,
                                            "op_count": op_count,
                                            "status": "skipped",
                                            "reason": (
                                                f"cloud_wallet_coin_op_error:{exc}"
                                                ":combine_for_split_prereq"
                                            ),
                                            "operation_id": None,
                                        }
                                    )
                                    split_submitted = True
                                    break
                                combine_sig_id = str(
                                    combine_result.get("signature_request_id", "")
                                ).strip()
                                if not combine_sig_id:
                                    items.append(
                                        {
                                            "op_type": op_type,
                                            "size_base_units": size_base_units,
                                            "op_count": op_count,
                                            "status": "skipped",
                                            "reason": "combine_missing_signature_request_id_for_split_prereq",
                                            "operation_id": None,
                                        }
                                    )
                                    split_submitted = True
                                    break
                                items.append(
                                    {
                                        "op_type": "combine",
                                        "size_base_units": size_base_units,
                                        "op_count": len(combine_input_coin_ids),
                                        "status": "executed",
                                        "reason": (
                                            "cloud_wallet_kms_combine_submitted_for_split_prereq_exact"
                                            if combine_exact_match
                                            else "cloud_wallet_kms_combine_submitted_for_split_prereq_with_change"
                                        ),
                                        "operation_id": combine_sig_id,
                                        "data": {
                                            "target_amount": required_amount,
                                            "selected_total": int(combine_selected_total),
                                            "exact_match": bool(combine_exact_match),
                                            "input_coin_cap_applied": bool(combine_cap_applied),
                                            "input_coin_cap": int(combine_input_cap),
                                            "selected_coin_count_before_cap": len(combine_coin_ids),
                                            "selected_coin_count_after_cap": len(
                                                combine_input_coin_ids
                                            ),
                                            "next_step_note": (
                                                "submitted capped progress combine; next cycle likely needs "
                                                "only 2-coin combine"
                                                if combine_cap_applied
                                                and combine_selected_total < required_amount
                                                else ""
                                            ),
                                        },
                                    }
                                )
                                executed_count += 1
                                split_submitted = True
                                break
                        break
                    selected_coin = max(fresh_spendable, key=lambda c: int(c.get("amount", 0)))
                    selected_coin_id = str(selected_coin.get("id", "")).strip()
                    if not selected_coin_id:
                        break
                    selected_amount = int(selected_coin.get("amount", 0))
                    selected_remainder = int(selected_amount - required_amount)
                    min_cat_mojos = _coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                    if (
                        min_cat_mojos > 0
                        and selected_remainder > 0
                        and selected_remainder < int(min_cat_mojos)
                    ):
                        items.append(
                            {
                                "op_type": op_type,
                                "size_base_units": size_base_units,
                                "op_count": op_count,
                                "status": "skipped",
                                "reason": "split_would_create_sub_cat_change",
                                "operation_id": None,
                                "data": {
                                    "selected_coin_id": selected_coin_id,
                                    "selected_amount_mojos": int(selected_amount),
                                    "required_amount_mojos": int(required_amount),
                                    "remainder_mojos": int(selected_remainder),
                                    "minimum_allowed_mojos": int(min_cat_mojos),
                                },
                            }
                        )
                        # Intentional: treat this op as "handled" for this cycle so
                        # we do not churn through alternate candidates and risk
                        # repeatedly planning dust-producing splits in one pass.
                        split_submitted = True
                        break
                    attempted_coin_ids.add(selected_coin_id)
                    try:
                        result = cloud_wallet.split_coins(
                            coin_ids=[selected_coin_id],
                            amount_per_coin=amount_per_coin_mojos,
                            number_of_coins=op_count,
                            fee=int(program.coin_ops_split_fee_mojos),
                        )
                    except Exception as exc:
                        error_text = str(exc)
                        if (
                            "Some selected coins are not spendable" in error_text
                            and attempt_index == 0
                        ):
                            continue
                        items.append(
                            {
                                "op_type": op_type,
                                "size_base_units": size_base_units,
                                "op_count": op_count,
                                "status": "skipped",
                                "reason": (
                                    f"cloud_wallet_coin_op_error:{exc}"
                                    f":selected_coin_id={selected_coin_id}"
                                ),
                                "operation_id": None,
                            }
                        )
                        split_submitted = True
                        break

                    signature_request_id = str(result.get("signature_request_id", "")).strip()
                    if not signature_request_id:
                        items.append(
                            {
                                "op_type": op_type,
                                "size_base_units": size_base_units,
                                "op_count": op_count,
                                "status": "skipped",
                                "reason": "split_missing_signature_request_id",
                                "operation_id": None,
                            }
                        )
                        split_submitted = True
                        break
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "executed",
                            "reason": "cloud_wallet_kms_split_submitted",
                            "operation_id": signature_request_id,
                        }
                    )
                    executed_count += 1
                    split_submitted = True
                    break

                if not split_submitted:
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "no_spendable_split_coin_meets_required_amount",
                            "operation_id": None,
                        }
                    )
                continue

            if op_type == "combine":
                requested_number_of_coins = max(2, op_count)
                capped_number_of_coins = min(requested_number_of_coins, combine_input_cap)
                target_coin_amount_mojos = size_base_units * base_unit_mojo_multiplier
                canonical_asset_id = str(getattr(market, "base_asset", "")).strip()
                # Defensive inner check — see comment in the split branch above.
                if not _coin_op_target_amount_allowed(
                    amount_mojos=target_coin_amount_mojos,
                    canonical_asset_id=canonical_asset_id,
                ):
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "combine_target_amount_below_coin_op_minimum",
                            "operation_id": None,
                            "data": {
                                "target_coin_amount_mojos": int(target_coin_amount_mojos),
                                "minimum_allowed_mojos": int(
                                    _coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)
                                ),
                            },
                        }
                    )
                    continue
                watched_coin_ids = _watched_coin_ids_for_market(
                    market_id=str(getattr(market, "market_id", "")).strip()
                )
                exact_bucket_coin_ids: list[str] = []
                for coin in _spendable_asset_scoped_coins(
                    cloud_wallet.list_coins(asset_id=resolved_base_asset_id, include_pending=True)
                ):
                    coin_id = str(coin.get("id", "")).strip()
                    if not coin_id or coin_id.lower() in watched_coin_ids:
                        continue
                    try:
                        amount_mojos = int(coin.get("amount", 0))
                    except (TypeError, ValueError):
                        continue
                    if amount_mojos != target_coin_amount_mojos:
                        continue
                    exact_bucket_coin_ids.append(coin_id)
                combine_input_coin_ids = exact_bucket_coin_ids[:capped_number_of_coins]
                if len(combine_input_coin_ids) < 2:
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "no_spendable_combine_coin_available",
                            "operation_id": None,
                        }
                    )
                    continue
                result = _combine_coins_with_retry(
                    cloud_wallet=cloud_wallet,
                    combine_kwargs={
                        "number_of_coins": len(combine_input_coin_ids),
                        "fee": int(program.coin_ops_combine_fee_mojos),
                        "asset_id": resolved_base_asset_id,
                        "largest_first": True,
                        "input_coin_ids": combine_input_coin_ids,
                    },
                )
                signature_request_id = str(result.get("signature_request_id", "")).strip()
                if not signature_request_id:
                    items.append(
                        {
                            "op_type": op_type,
                            "size_base_units": size_base_units,
                            "op_count": op_count,
                            "status": "skipped",
                            "reason": "combine_missing_signature_request_id",
                            "operation_id": None,
                        }
                    )
                    continue
                items.append(
                    {
                        "op_type": op_type,
                        "size_base_units": size_base_units,
                        "op_count": op_count,
                        "status": "executed",
                        "reason": "cloud_wallet_kms_combine_submitted",
                        "operation_id": signature_request_id,
                        "data": {
                            "requested_number_of_coins": int(requested_number_of_coins),
                            "submitted_number_of_coins": int(len(combine_input_coin_ids)),
                            "input_coin_cap_applied": bool(
                                capped_number_of_coins < requested_number_of_coins
                            ),
                            "input_coin_cap": int(combine_input_cap),
                            "input_coin_ids": combine_input_coin_ids,
                        },
                    }
                )
                executed_count += 1
                continue

            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": "invalid_plan",
                    "operation_id": None,
                }
            )
        except Exception as exc:
            items.append(
                {
                    "op_type": op_type,
                    "size_base_units": size_base_units,
                    "op_count": op_count,
                    "status": "skipped",
                    "reason": f"cloud_wallet_coin_op_error:{exc}",
                    "operation_id": None,
                }
            )

    return {
        "dry_run": bool(program.runtime_dry_run),
        "planned_count": len(plans),
        "executed_count": executed_count,
        "status": "cloud_wallet_kms",
        "signer_selection": {
            "selected_source": "signer_registry",
            "key_id": str(getattr(signer_selection, "key_id", "")).strip(),
            "network": str(getattr(program, "app_network", "")).strip(),
        },
        "items": items,
    }


def _process_single_market(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    store: SqliteStore,
    xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    cloud_wallet_scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> _MarketCycleResult:
    result = _MarketCycleResult()
    _log_market_decision(
        market.market_id,
        "cycle_start",
        mode=str(getattr(market, "mode", "")),
        quote_asset=str(getattr(market, "quote_asset", "")),
    )
    signer_selection = resolve_market_key(
        market,
        allowed_keys,
        signer_key_registry=program.signer_key_registry,
        required_network=program.app_network,
    )
    _log_market_decision(
        market.market_id,
        "signer_selected",
        key_id=signer_selection.key_id,
        network=program.app_network,
    )
    store.add_price_policy_snapshot(
        market.market_id,
        {
            "mode": market.mode,
            "base_asset": market.base_asset,
            "quote_asset": market.quote_asset,
            "quote_asset_type": market.quote_asset_type,
        },
        source="startup",
    )
    persisted = store.get_alert_state(market.market_id)
    state, event = evaluate_low_inventory_alert(
        now=now,
        program=program,
        market=market,
        state=AlertState(
            is_low=persisted.is_low,
            last_alert_at=persisted.last_alert_at,
        ),
    )
    store.upsert_alert_state(
        StoredAlertState(
            market_id=market.market_id,
            is_low=state.is_low,
            last_alert_at=state.last_alert_at,
        )
    )
    if event:
        payload = {
            "event": "low_inventory_alert",
            "market_id": event.market_id,
            "ticker": event.ticker,
            "remaining_amount": event.remaining_amount,
            "receive_address": event.receive_address,
            "reason": event.reason,
        }
        _log_daemon_event(level=logging.INFO, payload=payload)
        store.add_audit_event("low_inventory_alert", payload, market_id=market.market_id)
        send_pushover_alert(program, event)

    _, dexie_size_by_offer_id, _, offers = _reconcile_offer_states(
        market=market,
        network=program.app_network,
        dexie=dexie,
        store=store,
        now=now,
        result=result,
    )
    cancel_policy = _execute_cancel_policy_for_market(
        market=market,
        offers=offers,
        runtime_dry_run=program.runtime_dry_run,
        current_xch_price_usd=xch_price_usd,
        previous_xch_price_usd=previous_xch_price_usd,
        dexie=dexie,
        store=store,
    )
    if bool(cancel_policy.get("triggered", False)):
        result.cancel_triggered = True
    result.cancel_planned += int(cancel_policy.get("planned_count", 0))
    result.cancel_executed += int(cancel_policy.get("executed_count", 0))
    _log_market_decision(
        market.market_id,
        "cancel_policy_evaluated",
        eligible=cancel_policy["eligible"],
        triggered=cancel_policy["triggered"],
        reason=cancel_policy["reason"],
        move_bps=cancel_policy["move_bps"],
        threshold_bps=cancel_policy["threshold_bps"],
        planned_count=cancel_policy["planned_count"],
        executed_count=cancel_policy["executed_count"],
    )
    store.add_audit_event(
        "offer_cancel_policy",
        {
            "market_id": market.market_id,
            "eligible": cancel_policy["eligible"],
            "triggered": cancel_policy["triggered"],
            "reason": cancel_policy["reason"],
            "move_bps": cancel_policy["move_bps"],
            "threshold_bps": cancel_policy["threshold_bps"],
            "planned_count": cancel_policy["planned_count"],
            "executed_count": cancel_policy["executed_count"],
            "items": cancel_policy["items"],
        },
        market_id=market.market_id,
    )

    sell_ladder = market.ladders.get("sell", [])
    ladder_sizes = [e.size_base_units for e in sell_ladder]
    bucket_counts: dict[int, int] | None = None
    wallet_coins: list[int] = []
    cloud_wallet_scan_empty = False

    if _cloud_wallet_configured(program):
        try:
            cloud_wallet = _new_cloud_wallet_adapter_for_daemon(program)
            resolved_base_asset_id, _, _ = _resolve_cloud_wallet_offer_asset_ids_for_reservation(
                program=program,
                market=market,
                wallet=cloud_wallet,
            )
            wallet_coins = _cloud_wallet_spendable_base_unit_coin_amounts(
                wallet=cloud_wallet,
                resolved_asset_id=resolved_base_asset_id,
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
                canonical_asset_id=str(market.base_asset),
                scoped_list_cache=cloud_wallet_scoped_list_cache,
            )
            cloud_wallet_scan_empty = len(wallet_coins) == 0
            bucket_counts = compute_bucket_counts_from_coins(
                coin_amounts_base_units=wallet_coins,
                ladder_sizes=ladder_sizes,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_wallet",
                source="cloud_wallet",
                resolved_asset_id=resolved_base_asset_id,
                coin_count=len(wallet_coins),
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": "cloud_wallet",
                    "resolved_asset_id": resolved_base_asset_id,
                    "bucket_counts": bucket_counts,
                    "coin_count": len(wallet_coins),
                },
                market_id=market.market_id,
            )
        except Exception as exc:
            _daemon_logger.warning(
                "cloud_wallet_inventory_scan_failed market_id=%s error=%s",
                market.market_id,
                exc,
            )

    if bucket_counts is None or cloud_wallet_scan_empty:
        fallback_source = (
            "wallet_adapter_fallback_after_empty_cloud_wallet_scan"
            if cloud_wallet_scan_empty
            else "wallet_adapter"
        )
        if cloud_wallet_scan_empty and str(market.base_asset).strip().lower() not in {
            "xch",
            "1",
            "",
        }:
            wallet_coins = _coinset_cat_spendable_base_unit_coin_amounts(
                canonical_asset_id=str(market.base_asset),
                receive_address=str(market.receive_address),
                network=str(program.app_network),
                base_unit_mojo_multiplier=_base_unit_mojo_multiplier_for_market(market=market),
            )
            if wallet_coins:
                fallback_source = "coinset_cat_scan_fallback_after_empty_cloud_wallet_scan"
        if not wallet_coins:
            wallet_coins = wallet.list_asset_coins_base_units(
                asset_id=market.base_asset,
                key_id=market.signer_key_id,
                receive_address=market.receive_address,
                network=program.app_network,
            )
        if wallet_coins:
            bucket_counts = compute_bucket_counts_from_coins(
                coin_amounts_base_units=wallet_coins,
                ladder_sizes=ladder_sizes,
            )
            _log_market_decision(
                market.market_id,
                "inventory_scan_wallet",
                source=fallback_source,
                coin_count=len(wallet_coins),
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": fallback_source,
                    "bucket_counts": bucket_counts,
                    "coin_count": len(wallet_coins),
                },
                market_id=market.market_id,
            )
        else:
            bucket_counts = dict(market.inventory.bucket_counts)
            _log_market_decision(
                market.market_id,
                "inventory_scan_config_fallback",
                asset_id=market.base_asset,
                bucket_counts=bucket_counts,
            )
            store.add_audit_event(
                "inventory_bucket_scan",
                {
                    "market_id": market.market_id,
                    "source": "config_seed_or_no_asset_scan",
                    "asset_id": market.base_asset,
                    "bucket_counts": bucket_counts,
                },
                market_id=market.market_id,
            )
    offer_counts_by_side: dict[str, dict[int, int]] = {"buy": {}, "sell": {}}
    newly_executed_sell_offer_counts_by_size: dict[int, int] = {}
    try:
        offer_counts_by_side, newly_executed_sell_offer_counts_by_size = (
            _evaluate_and_execute_strategy(
                market=market,
                program=program,
                dexie=dexie,
                splash=splash,
                store=store,
                xch_price_usd=xch_price_usd,
                now=now,
                dexie_size_by_offer_id=dexie_size_by_offer_id,
                result=result,
                reservation_coordinator=reservation_coordinator,
                cloud_wallet_scoped_list_cache=cloud_wallet_scoped_list_cache,
            )
        )
    except Exception as exc:
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "strategy_failed",
            error=str(exc),
        )
        store.add_audit_event(
            "strategy_execution_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
    try:
        _plan_and_execute_coin_ops(
            market=market,
            program=program,
            wallet=wallet,
            store=store,
            sell_ladder=sell_ladder,
            wallet_bucket_counts=bucket_counts,
            active_sell_offer_counts_by_size=offer_counts_by_side.get("sell", {}),
            newly_executed_sell_offer_counts_by_size=newly_executed_sell_offer_counts_by_size,
            signer_selection=signer_selection,
            state_dir=state_dir,
        )
    except Exception as exc:
        result.cycle_errors += 1
        _log_market_decision(
            market.market_id,
            "coin_ops_failed",
            error=str(exc),
        )
        store.add_audit_event(
            "coin_ops_execution_error",
            {"market_id": market.market_id, "error": str(exc)},
            market_id=market.market_id,
        )
    _log_market_decision(
        market.market_id,
        "cycle_complete",
        cycle_errors=result.cycle_errors,
        strategy_planned=result.strategy_planned,
        strategy_executed=result.strategy_executed,
        cancel_triggered=result.cancel_triggered,
        cancel_planned=result.cancel_planned,
        cancel_executed=result.cancel_executed,
    )
    return result


def _process_single_market_with_store(
    *,
    market: Any,
    program: Any,
    allowed_keys: set[str] | None,
    dexie: DexieAdapter,
    splash: SplashAdapter,
    wallet: WalletAdapter,
    db_path: Path,
    xch_price_usd: float | None,
    previous_xch_price_usd: float | None,
    now: datetime,
    state_dir: Path,
    reservation_coordinator: AssetReservationCoordinator | None = None,
    cloud_wallet_scoped_list_cache: CloudWalletAssetScopedListCache | None = None,
) -> _MarketCycleResult:
    """Run one market cycle with a thread-local SQLite connection."""
    store = SqliteStore(db_path)
    try:
        return _process_single_market(
            market=market,
            program=program,
            allowed_keys=allowed_keys,
            dexie=dexie,
            splash=splash,
            wallet=wallet,
            store=store,
            xch_price_usd=xch_price_usd,
            previous_xch_price_usd=previous_xch_price_usd,
            now=now,
            state_dir=state_dir,
            reservation_coordinator=reservation_coordinator,
            cloud_wallet_scoped_list_cache=cloud_wallet_scoped_list_cache,
        )
    finally:
        store.close()


def run_once(
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
    poll_coinset_mempool: bool = True,
    use_websocket_capture: bool = False,
    program=None,
    testnet_markets_path: Path | None = None,
    market_dispatch_state: _MarketDispatchState | None = None,
) -> int:
    if program is None:
        program = load_program_config(program_path)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
    _log_disabled_markets_startup_once(markets=list(markets.markets))
    db_path = _resolve_db_path(program.home_dir, db_path_override)
    store = SqliteStore(db_path)
    started_at = time.monotonic()

    try:
        markets_processed = 0
        markets_attempted = 0
        cycle_error_count = 0
        strategy_planned_total = 0
        strategy_executed_total = 0
        cancel_triggered_count = 0
        cancel_planned_total = 0
        cancel_executed_total = 0
        dexie = DexieAdapter(program.dexie_api_base)
        splash = SplashAdapter(program.splash_api_base)
        wallet = WalletAdapter()
        cloud_wallet_price_fn = None
        if _cloud_wallet_configured(program):
            try:
                cloud_wallet_price_fn = _new_cloud_wallet_adapter_for_daemon(
                    program
                ).get_chia_usd_quote
            except Exception as exc:
                store.add_audit_event(
                    "xch_price_provider_init_error",
                    {"provider": "cloud_wallet_quote", "error": str(exc)},
                )
        price = XchPriceProvider(
            cloud_wallet_price_fn=cloud_wallet_price_fn,
            cloud_wallet_ttl_seconds=120,
            fallback_price_adapter=PriceAdapter(),
        )
        previous_xch_price_usd = store.get_latest_xch_price_snapshot()
        reservation_coordinator: AssetReservationCoordinator | None = None
        if bool(
            getattr(program, "runtime_offer_parallelism_enabled", False)
        ) and _cloud_wallet_configured(program):
            reservation_coordinator = AssetReservationCoordinator(
                db_path=db_path,
                lease_seconds=int(getattr(program, "runtime_reservation_ttl_seconds", 300)),
            )
            expired_count = reservation_coordinator.expire_stale()
            if expired_count > 0:
                store.add_audit_event("reservation_expired", {"count": int(expired_count)})
        xch_price_usd: float | None = None
        try:
            xch_price_usd = asyncio.run(price.get_xch_price())
            store.add_audit_event("xch_price_snapshot", {"price_usd": xch_price_usd})
        except Exception as exc:  # pragma: no cover - network dependent
            cycle_error_count += 1
            store.add_audit_event("xch_price_error", {"error": str(exc)})
        if use_websocket_capture:
            try:
                _run_coinset_signal_capture_once(
                    program=program,
                    coinset_base_url=coinset_base_url,
                    store=store,
                )
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event("coinset_ws_once_error", {"error": str(exc)})
        elif poll_coinset_mempool:
            try:
                coinset = _build_coinset_adapter(program=program, coinset_base_url=coinset_base_url)
                tx_ids = coinset.get_all_mempool_tx_ids()
                new_count = store.observe_mempool_tx_ids(tx_ids)
                store.add_audit_event("coinset_mempool_snapshot", {"count": len(tx_ids)})
                if new_count:
                    store.add_audit_event("mempool_observed", {"new_tx_ids": new_count})
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event("coinset_mempool_error", {"error": str(exc)})

        now = utcnow()
        enabled_markets: list[Any] = []
        for market in markets.markets:
            if not market.enabled:
                if _should_log_disabled_market(market_id=market.market_id):
                    _log_market_decision(market.market_id, "market_skipped", reason="disabled")
                continue
            _DISABLED_MARKET_NEXT_LOG_AT.pop(market.market_id, None)
            enabled_markets.append(market)

        stale_open_sweep_payload: dict[str, Any] = {
            "checked_offer_count": 0,
            "requeue_market_ids": [],
            "hits": [],
            "truncated": False,
        }
        if enabled_markets:
            stale_open_sweep_payload = _detect_stale_open_offers_for_requeue(
                store=store,
                dexie=dexie,
                enabled_market_ids={
                    str(getattr(market, "market_id", "")).strip() for market in enabled_markets
                },
            )
            stale_requeues = [
                str(mid).strip()
                for mid in stale_open_sweep_payload.get("requeue_market_ids", [])
                if str(mid).strip()
            ]
            if market_dispatch_state is not None:
                for market_id in stale_requeues:
                    _enqueue_immediate_requeue_market(market_dispatch_state, market_id)
            if stale_requeues:
                store.add_audit_event(
                    "stale_open_offer_requeue_detected",
                    {
                        "market_ids": stale_requeues,
                        "checked_offer_count": int(
                            stale_open_sweep_payload.get("checked_offer_count", 0)
                        ),
                        "truncated": bool(stale_open_sweep_payload.get("truncated", False)),
                        "hits": list(stale_open_sweep_payload.get("hits", []))[:50],
                    },
                )

        configured_market_slot_count = int(getattr(program, "runtime_market_slot_count", 0))
        consumed_immediate_requeues: list[str] = []
        if (
            market_dispatch_state is not None
            and configured_market_slot_count > 0
            and len(enabled_markets) > configured_market_slot_count
        ):
            selected_markets, consumed_immediate_requeues = _select_market_batch(
                enabled_markets=enabled_markets,
                slot_count=configured_market_slot_count,
                dispatch_state=market_dispatch_state,
            )
            _daemon_logger.info(
                "market_slot_dispatch enabled=true slot_count=%s selected=%s enabled=%s immediate_requeue_consumed=%s cursor=%s pending_requeues=%s",
                configured_market_slot_count,
                len(selected_markets),
                len(enabled_markets),
                len(consumed_immediate_requeues),
                market_dispatch_state.cursor,
                len(market_dispatch_state.immediate_requeue_ids),
            )
        else:
            selected_markets = enabled_markets
            if market_dispatch_state is not None and enabled_markets:
                # Keep scheduler cursor bounded even when slot dispatch is disabled.
                market_dispatch_state.cursor %= len(enabled_markets)
        markets_attempted = len(selected_markets)
        immediate_requeue_market_ids: list[str] = []
        cloud_wallet_scoped_list_cache: CloudWalletAssetScopedListCache | None = None
        if _cloud_wallet_configured(program):
            try:
                cloud_wallet_scoped_list_cache = CloudWalletAssetScopedListCache(
                    _new_cloud_wallet_adapter_for_daemon(program)
                )
            except Exception:
                cloud_wallet_scoped_list_cache = None
        if bool(getattr(program, "runtime_parallel_markets", False)) and len(selected_markets) > 1:
            max_workers = max(1, len(selected_markets))
            _daemon_logger.info(
                "market_parallel_dispatch enabled=true workers=%s markets=%s",
                max_workers,
                markets_attempted,
            )
            with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
                future_to_market = {
                    pool.submit(
                        _process_single_market_with_store,
                        market=market,
                        program=program,
                        allowed_keys=allowed_keys,
                        dexie=dexie,
                        splash=splash,
                        wallet=wallet,
                        db_path=db_path,
                        xch_price_usd=xch_price_usd,
                        previous_xch_price_usd=previous_xch_price_usd,
                        now=now,
                        state_dir=state_dir,
                        reservation_coordinator=reservation_coordinator,
                        cloud_wallet_scoped_list_cache=cloud_wallet_scoped_list_cache,
                    ): market
                    for market in selected_markets
                }
                for future in concurrent.futures.as_completed(future_to_market):
                    market = future_to_market[future]
                    market_id = str(getattr(market, "market_id", "")).strip()
                    try:
                        mr = future.result()
                    except Exception as exc:
                        cycle_error_count += 1
                        _log_market_decision(
                            market_id or "unknown",
                            "cycle_failed",
                            error=str(exc),
                        )
                        # This runs in the main thread while iterating
                        # `as_completed`, so emitting the aggregate market-cycle
                        # error through the outer store is thread-safe.
                        store.add_audit_event(
                            "market_cycle_error",
                            {
                                "market_id": market_id,
                                "error": str(exc),
                                "source": "parallel_market_worker",
                            },
                        )
                        continue
                    markets_processed += 1
                    cycle_error_count += mr.cycle_errors
                    strategy_planned_total += mr.strategy_planned
                    strategy_executed_total += mr.strategy_executed
                    if mr.cancel_triggered:
                        cancel_triggered_count += 1
                    cancel_planned_total += mr.cancel_planned
                    cancel_executed_total += mr.cancel_executed
                    if mr.immediate_requeue_requested and market_id:
                        immediate_requeue_market_ids.append(market_id)
        else:
            _daemon_logger.info(
                "market_parallel_dispatch enabled=false workers=1 markets=%s",
                markets_attempted,
            )
            for market in selected_markets:
                market_id = str(getattr(market, "market_id", "")).strip()
                try:
                    mr = _process_single_market(
                        market=market,
                        program=program,
                        allowed_keys=allowed_keys,
                        dexie=dexie,
                        splash=splash,
                        wallet=wallet,
                        store=store,
                        xch_price_usd=xch_price_usd,
                        previous_xch_price_usd=previous_xch_price_usd,
                        now=now,
                        state_dir=state_dir,
                        reservation_coordinator=reservation_coordinator,
                        cloud_wallet_scoped_list_cache=cloud_wallet_scoped_list_cache,
                    )
                except Exception as exc:
                    cycle_error_count += 1
                    _log_market_decision(
                        market_id or "unknown",
                        "cycle_failed",
                        error=str(exc),
                    )
                    store.add_audit_event(
                        "market_cycle_error",
                        {
                            "market_id": market_id,
                            "error": str(exc),
                            "source": "sequential_market_worker",
                        },
                    )
                    continue
                markets_processed += 1
                cycle_error_count += mr.cycle_errors
                strategy_planned_total += mr.strategy_planned
                strategy_executed_total += mr.strategy_executed
                if mr.cancel_triggered:
                    cancel_triggered_count += 1
                cancel_planned_total += mr.cancel_planned
                cancel_executed_total += mr.cancel_executed
                if mr.immediate_requeue_requested and market_id:
                    immediate_requeue_market_ids.append(market_id)
        deduped_requeue_market_ids = sorted({mid for mid in immediate_requeue_market_ids if mid})
        if market_dispatch_state is not None:
            for market_id in deduped_requeue_market_ids:
                _enqueue_immediate_requeue_market(market_dispatch_state, market_id)
        duration_ms = int((time.monotonic() - started_at) * 1000)
        store.add_audit_event(
            "daemon_cycle_summary",
            {
                "duration_ms": duration_ms,
                "enabled_markets": len(enabled_markets),
                "markets_attempted": markets_attempted,
                "markets_processed": markets_processed,
                "runtime_market_slot_count": configured_market_slot_count,
                "stale_open_sweep_checked_offer_count": int(
                    stale_open_sweep_payload.get("checked_offer_count", 0)
                ),
                "stale_open_sweep_requeue_market_ids": list(
                    stale_open_sweep_payload.get("requeue_market_ids", [])
                ),
                "stale_open_sweep_requeue_count": len(
                    list(stale_open_sweep_payload.get("requeue_market_ids", []))
                ),
                "stale_open_sweep_truncated": bool(
                    stale_open_sweep_payload.get("truncated", False)
                ),
                "immediate_requeue_market_ids": deduped_requeue_market_ids,
                "immediate_requeue_count": len(deduped_requeue_market_ids),
                "error_count": cycle_error_count,
                "strategy_planned_total": strategy_planned_total,
                "strategy_executed_total": strategy_executed_total,
                "cancel_triggered_count": cancel_triggered_count,
                "cancel_planned_total": cancel_planned_total,
                "cancel_executed_total": cancel_executed_total,
            },
        )
        return 0
    finally:
        store.close()


def _run_loop(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    current_program = load_program_config(program_path)
    market_dispatch_state = _MarketDispatchState()
    _initialize_daemon_file_logging(
        current_program.home_dir, log_level=getattr(current_program, "app_log_level", "INFO")
    )
    _warn_if_log_level_auto_healed(program=current_program, program_path=program_path)
    _daemon_logger.info(
        "daemon_starting mode=loop program_config=%s markets_config=%s",
        os.fspath(program_path),
        os.fspath(markets_path),
    )
    db_path = _resolve_db_path(current_program.home_dir, db_path_override)
    coinset = _build_coinset_adapter(program=current_program, coinset_base_url=coinset_base_url)
    ws_url = _resolve_coinset_ws_url(program=current_program, coinset_base_url=coinset_base_url)

    def _with_ws_store(callback: Callable[[SqliteStore], None]) -> None:
        # Websocket callbacks may run on a worker thread, so open a
        # callback-local SQLite connection instead of reusing a main-thread store.
        store = SqliteStore(db_path)
        try:
            callback(store)
        finally:
            store.close()

    def _on_mempool_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            new_count = store.observe_mempool_tx_ids(tx_ids)
            if new_count:
                store.add_audit_event(
                    "mempool_observed",
                    {"new_tx_ids": new_count, "source": "coinset_websocket"},
                )

        _with_ws_store(_write)

    def _on_confirmed_tx_ids(tx_ids: list[str]) -> None:
        if not tx_ids:
            return

        def _write(store: SqliteStore) -> None:
            confirmed = store.confirm_tx_ids(tx_ids)
            store.add_audit_event(
                "tx_block_confirmed",
                {
                    "tx_ids": tx_ids,
                    "confirmed_count": confirmed,
                    "source": "coinset_websocket",
                },
            )

        _with_ws_store(_write)

    def _on_audit_event(event_type: str, payload: dict[str, Any]) -> None:
        _with_ws_store(lambda store: store.add_audit_event(event_type, payload))

    def _on_observed_coin_ids(coin_ids: list[str]) -> None:
        if not coin_ids:
            return
        hits = _match_watched_coin_ids(observed_coin_ids=coin_ids)
        if not hits:
            return

        def _write(store: SqliteStore) -> None:
            store.add_audit_event(
                "coin_watch_hit",
                {
                    "coin_id_count": len(coin_ids),
                    "coin_ids_sample": sorted({str(c).strip().lower() for c in coin_ids})[:10],
                    "market_hits": {market_id: ids[:10] for market_id, ids in hits.items()},
                    "source": "coinset_websocket",
                },
            )

        _with_ws_store(_write)

    ws_client = CoinsetWebsocketClient(
        ws_url=ws_url,
        reconnect_interval_seconds=current_program.tx_block_websocket_reconnect_interval_seconds,
        on_mempool_tx_ids=_on_mempool_tx_ids,
        on_confirmed_tx_ids=_on_confirmed_tx_ids,
        on_audit_event=_on_audit_event,
        on_observed_coin_ids=_on_observed_coin_ids,
        recovery_poll=coinset.get_all_mempool_tx_ids,
    )
    ws_client.start()

    try:
        while True:
            _initialize_daemon_file_logging(
                current_program.home_dir,
                log_level=getattr(current_program, "app_log_level", "INFO"),
            )
            _warn_if_log_level_auto_healed(program=current_program, program_path=program_path)
            run_once(
                program_path=program_path,
                markets_path=markets_path,
                testnet_markets_path=testnet_markets_path,
                allowed_keys=allowed_keys,
                db_path_override=db_path_override,
                coinset_base_url=coinset_base_url,
                state_dir=state_dir,
                poll_coinset_mempool=False,
                program=current_program,
                market_dispatch_state=market_dispatch_state,
            )
            if _consume_reload_marker(state_dir):
                _log_daemon_event(level=logging.INFO, payload={"event": "config_reloaded"})
            time.sleep(max(1, current_program.runtime_loop_interval_seconds))
            current_program = load_program_config(program_path)
    except KeyboardInterrupt:
        return 0
    finally:
        ws_client.stop()
        _daemon_logger.info("daemon_stopped mode=loop")


def main() -> None:
    def _default_testnet_markets_config_path() -> str:
        candidate = Path("~/.greenfloor/config/testnet-markets.yaml").expanduser()
        if candidate.exists():
            return str(candidate)
        return ""

    parser = argparse.ArgumentParser(description="Run GreenFloor daemon")
    parser.add_argument(
        "--program-config",
        default="config/program.yaml",
        help="Path to program.yaml",
    )
    parser.add_argument(
        "--markets-config",
        default="config/markets.yaml",
        help="Path to markets.yaml",
    )
    parser.add_argument(
        "--testnet-markets-config",
        default=_default_testnet_markets_config_path(),
        help=(
            "Optional path to testnet-markets.yaml overlay. "
            "Ignored when unset or file does not exist."
        ),
    )
    parser.add_argument(
        "--key-ids",
        default="",
        help="Comma-separated signer key IDs allowed for this daemon instance",
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run one evaluation cycle and exit",
    )
    parser.add_argument("--state-db", default="", help="Optional explicit SQLite state DB path")
    parser.add_argument(
        "--coinset-base-url",
        default="https://api.coinset.org",
        help="Coinset API base URL",
    )
    parser.add_argument(
        "--state-dir",
        default=str(default_state_dir_path()),
        help="State directory used for reload marker and daemon-local state",
    )
    args = parser.parse_args()
    state_dir = Path(args.state_dir).expanduser()
    testnet_markets_path = (
        Path(args.testnet_markets_config) if str(args.testnet_markets_config).strip() else None
    )

    allowed_keys = {k.strip() for k in args.key_ids.split(",") if k.strip()} or None
    try:
        with _acquire_daemon_instance_lock(
            state_dir=state_dir,
            mode="once" if args.once else "loop",
        ):
            if args.once:
                program = load_program_config(Path(args.program_config))
                _initialize_daemon_file_logging(
                    program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
                )
                _warn_if_log_level_auto_healed(
                    program=program, program_path=Path(args.program_config)
                )
                _daemon_logger.info(
                    "daemon_starting mode=once program_config=%s markets_config=%s",
                    args.program_config,
                    args.markets_config,
                )
                exit_code = run_once(
                    Path(args.program_config),
                    Path(args.markets_config),
                    allowed_keys,
                    args.state_db or None,
                    args.coinset_base_url,
                    state_dir,
                    poll_coinset_mempool=False,
                    use_websocket_capture=program.tx_block_trigger_mode == "websocket",
                    testnet_markets_path=testnet_markets_path,
                )
                _daemon_logger.info("daemon_stopped mode=once exit_code=%s", exit_code)
            else:
                exit_code = _run_loop(
                    program_path=Path(args.program_config),
                    markets_path=Path(args.markets_config),
                    testnet_markets_path=testnet_markets_path,
                    allowed_keys=allowed_keys,
                    db_path_override=args.state_db or None,
                    coinset_base_url=args.coinset_base_url,
                    state_dir=state_dir,
                )
    except RuntimeError as exc:
        try:
            program = load_program_config(Path(args.program_config))
            _initialize_daemon_file_logging(
                program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
            )
            _warn_if_log_level_auto_healed(program=program, program_path=Path(args.program_config))
        except Exception:
            pass
        _log_daemon_event(
            level=logging.ERROR,
            payload={"event": "daemon_lock_conflict", "error": str(exc)},
        )
        raise SystemExit(3) from exc
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
