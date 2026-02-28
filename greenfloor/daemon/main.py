from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import threading
import time
import urllib.parse
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import yaml
from concurrent_log_handler import ConcurrentRotatingFileHandler

from greenfloor.adapters.coinset import (
    CoinsetAdapter,
    extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
)
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
)
from greenfloor.core.coin_ops import BucketSpec, plan_coin_ops
from greenfloor.core.fee_budget import partition_plans_by_budget, projected_coin_ops_fee_mojos
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert, utcnow
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.core.strategy import MarketState, PlannedAction, StrategyConfig, evaluate_market
from greenfloor.daemon.coinset_ws import CoinsetWebsocketClient, capture_coinset_websocket_once
from greenfloor.keys.router import resolve_market_key
from greenfloor.logging_setup import (
    apply_level_to_root,
    coerce_log_level,
    create_rotating_file_handler,
)
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState

_DEFAULT_CANCEL_MOVE_THRESHOLD_BPS = 500
_POST_COOLDOWN_UNTIL: dict[str, float] = {}
_CANCEL_COOLDOWN_UNTIL: dict[str, float] = {}
_DAEMON_SERVICE_NAME = "daemon"
_daemon_file_logger_initialized = False
_daemon_file_log_handler: ConcurrentRotatingFileHandler | None = None
_daemon_logger = logging.getLogger("greenfloor.daemon")
_DISABLED_MARKET_LOG_INTERVAL_SECONDS_DEFAULT = 3600
_DISABLED_MARKET_NEXT_LOG_AT: dict[str, float] = {}
_DISABLED_MARKET_STARTUP_LOGGED = False
_WATCHED_COIN_IDS_BY_MARKET: dict[str, set[str]] = {}
_WATCHED_COIN_IDS_LOCK = threading.Lock()


def _log_market_decision(market_id: str, decision: str, **fields: Any) -> None:
    extras = " ".join(f"{key}={fields[key]}" for key in sorted(fields))
    if extras:
        _daemon_logger.info(
            "market_decision market_id=%s decision=%s %s", market_id, decision, extras
        )
    else:
        _daemon_logger.info("market_decision market_id=%s decision=%s", market_id, decision)


def _initialize_daemon_file_logging(home_dir: str, *, log_level: str | None) -> None:
    global _daemon_file_logger_initialized, _daemon_file_log_handler
    root_logger = logging.getLogger()
    effective_level = coerce_log_level(log_level)
    if not _daemon_file_logger_initialized:
        handler = create_rotating_file_handler(service_name=_DAEMON_SERVICE_NAME, home_dir=home_dir)
        root_logger.addHandler(handler)
        _daemon_file_log_handler = handler
        _daemon_file_logger_initialized = True
    apply_level_to_root(
        effective_level=effective_level,
        logger=_daemon_logger,
        handler=_daemon_file_log_handler,
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
    if bool(getattr(program, "app_log_level_was_missing", False)):
        _daemon_logger.warning(
            "program config missing app.log_level; wrote default INFO to %s",
            os.fspath(program_path),
        )


def _consume_reload_marker(state_dir: Path) -> bool:
    marker = state_dir / "reload_request.json"
    if not marker.exists():
        return False
    marker.unlink(missing_ok=True)
    return True


def _resolve_db_path(program_home_dir: str, explicit_db_path: str | None) -> Path:
    if explicit_db_path:
        return Path(explicit_db_path).expanduser()
    return (Path(program_home_dir).expanduser() / "db" / "greenfloor.sqlite").resolve()


def _cancel_move_threshold_bps() -> int:
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


def _cooldown_remaining_ms(cooldowns: dict[str, float], key: str) -> int:
    deadline = float(cooldowns.get(key, 0.0))
    remaining = max(0.0, deadline - time.monotonic())
    return int(remaining * 1000)


def _set_cooldown(cooldowns: dict[str, float], key: str, cooldown_seconds: int) -> None:
    if cooldown_seconds <= 0:
        return
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
        is_success=lambda r: bool(r.get("success", False)) and bool(str(r.get("id", "")).strip()),
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
        is_success=lambda r: bool(r.get("success", False)),
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
    normalized = value.strip().lower()
    return len(normalized) == 64 and all(ch in "0123456789abcdef" for ch in normalized)


def _default_cats_config_path() -> Path | None:
    home_candidate = Path("~/.greenfloor/config/cats.yaml").expanduser()
    if home_candidate.exists():
        return home_candidate
    repo_candidate = Path("config/cats.yaml")
    if repo_candidate.exists():
        return repo_candidate
    return None


def _resolve_quote_asset_for_offer(*, quote_asset: str, network: str) -> str:
    normalized = quote_asset.strip().lower()
    if normalized in {"xch", "txch", "1"}:
        if network.strip().lower() in {"testnet", "testnet11"}:
            return "txch"
        return "xch"
    if _is_hex_asset_id(normalized):
        return normalized

    cats_path = _default_cats_config_path()
    if cats_path is None:
        return quote_asset
    try:
        raw = yaml.safe_load(cats_path.read_text(encoding="utf-8")) or {}
    except Exception:
        return quote_asset
    if not isinstance(raw, dict):
        return quote_asset
    cats = raw.get("cats", [])
    if not isinstance(cats, list):
        return quote_asset
    for item in cats:
        if not isinstance(item, dict):
            continue
        symbol = str(item.get("base_symbol", "")).strip().lower()
        if symbol != normalized:
            continue
        asset_id = str(item.get("asset_id", "")).strip().lower()
        if _is_hex_asset_id(asset_id):
            return asset_id
    return quote_asset


def _market_pricing(market: Any) -> dict[str, Any]:
    return dict(getattr(market, "pricing", {}) or {})


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

    return StrategyConfig(
        pair=_normalize_strategy_pair(market.quote_asset),
        ones_target=int(targets_by_size.get(1, 5)),
        tens_target=int(targets_by_size.get(10, 2)),
        hundreds_target=int(targets_by_size.get(100, 1)),
        target_spread_bps=_to_int(pricing.get("strategy_target_spread_bps")),
        min_xch_price_usd=_to_float(pricing.get("strategy_min_xch_price_usd")),
        max_xch_price_usd=_to_float(pricing.get("strategy_max_xch_price_usd")),
        offer_expiry_unit=str(pricing.get("strategy_offer_expiry_unit", "")).strip().lower()
        or None,
        offer_expiry_value=_to_int(pricing.get("strategy_offer_expiry_value")),
    )


def _strategy_state_from_bucket_counts(
    bucket_counts: dict[int, int],
    *,
    xch_price_usd: float | None,
) -> MarketState:
    return MarketState(
        ones=int(bucket_counts.get(1, 0)),
        tens=int(bucket_counts.get(10, 0)),
        hundreds=int(bucket_counts.get(100, 0)),
        xch_price_usd=xch_price_usd,
    )


_ACTIVE_OFFER_STATES_FOR_RESEED = {
    OfferLifecycleState.OPEN.value,
    OfferLifecycleState.REFRESH_DUE.value,
}
_RESEED_MEMPOOL_MAX_AGE_SECONDS = 3 * 60


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
        updated_at = updated_at.replace(tzinfo=UTC)
    age_seconds = (clock - updated_at).total_seconds()
    return 0 <= age_seconds <= float(max_age_seconds)


def _strategy_target_counts_by_size(strategy_config: StrategyConfig) -> dict[int, int]:
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


def _active_offer_counts_by_size(
    *,
    store: SqliteStore,
    market_id: str,
    clock: datetime,
    limit: int = 500,
) -> tuple[dict[int, int], dict[str, int], int]:
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
    size_by_offer_id = _recent_offer_sizes_by_offer_id(store=store, market_id=market_id)
    active_counts_by_size: dict[int, int] = {1: 0, 10: 0, 100: 0}
    active_unmapped_offer_ids = 0
    for offer_id in active_offer_ids:
        size = size_by_offer_id.get(offer_id)
        if size in active_counts_by_size:
            active_counts_by_size[size] = int(active_counts_by_size[size]) + 1
        else:
            active_unmapped_offer_ids += 1
    return active_counts_by_size, state_counts, active_unmapped_offer_ids


def _inject_reseed_action_if_no_active_offers(
    *,
    strategy_actions: list[PlannedAction],
    strategy_config: StrategyConfig,
    market,
    store: SqliteStore,
    xch_price_usd: float | None,
    clock: datetime,
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
        state=MarketState(ones=0, tens=0, hundreds=0, xch_price_usd=xch_price_usd),
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
    from greenfloor.cli.offer_builder_sdk import build_offer_text

    pricing = _market_pricing(market)
    try:
        quote_price = _resolve_quote_price_quote_per_base(market)
    except Exception as exc:
        return {
            "status": "skipped",
            "reason": f"offer_builder_failed:{exc}",
            "offer": None,
        }
    payload = {
        "market_id": market.market_id,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": _resolve_quote_asset_for_offer(
            quote_asset=str(market.quote_asset),
            network=network,
        ),
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "size_base_units": int(action.size),
        "pair": action.pair,
        "reason": action.reason,
        "xch_price_usd": xch_price_usd,
        "target_spread_bps": action.target_spread_bps,
        "expiry_unit": action.expiry_unit,
        "expiry_value": int(action.expiry_value),
        "quote_price_quote_per_base": quote_price,
        "base_unit_mojo_multiplier": int(pricing.get("base_unit_mojo_multiplier", 1000)),
        "quote_unit_mojo_multiplier": int(pricing.get("quote_unit_mojo_multiplier", 1000)),
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


def _cloud_wallet_offer_post_fallback(
    *,
    program: Any,
    market: Any,
    size_base_units: int,
    publish_venue: str,
    runtime_dry_run: bool,
) -> dict[str, Any]:
    from greenfloor.cli.manager import _build_and_post_offer_cloud_wallet

    quote_price = _resolve_quote_price_quote_per_base(market)
    exit_code, payload = _build_and_post_offer_cloud_wallet(
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
    )
    if exit_code != 0:
        results = payload.get("results", [])
        result = (
            results[0].get("result", {})
            if isinstance(results, list) and results and isinstance(results[0], dict)
            else {}
        )
        error = str(result.get("error", "")).strip() if isinstance(result, dict) else ""
        return {"success": False, "error": error or f"cloud_wallet_fallback_exit_code:{exit_code}"}
    results = payload.get("results", [])
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
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    executed_count = 0
    _, _, cooldown_seconds = _post_retry_config()
    cooldown_key = f"{publish_venue}:{market.market_id}"
    signer_key = (signer_key_registry or {}).get(market.signer_key_id)
    keyring_yaml_path = str(getattr(signer_key, "keyring_yaml_path", "") or "")
    # Prioritize larger ladder sizes first to reduce input-coin contention in
    # cloud-wallet sequential posting (e.g. keep 100-size offers from being
    # displaced by a burst of 1-size posts in the same cycle).
    ordered_actions = sorted(strategy_actions, key=lambda action: int(action.size), reverse=True)
    for action in ordered_actions:
        for _ in range(int(action.repeat)):
            if runtime_dry_run:
                items.append(
                    {
                        "size": action.size,
                        "status": "planned",
                        "reason": "dry_run",
                        "offer_id": None,
                    }
                )
                continue

            if program is not None and _cloud_wallet_configured(program):
                cloud_wallet_post = _cloud_wallet_offer_post_fallback(
                    program=program,
                    market=market,
                    size_base_units=int(action.size),
                    publish_venue=publish_venue,
                    runtime_dry_run=runtime_dry_run,
                )
                if bool(cloud_wallet_post.get("success", False)):
                    cloud_wallet_offer_id = str(cloud_wallet_post.get("offer_id", "")).strip()
                    if publish_venue == "dexie" and cloud_wallet_offer_id:
                        visible, visibility_error = _verify_offer_visible_on_dexie(
                            dexie=dexie,
                            offer_id=cloud_wallet_offer_id,
                        )
                        if not visible:
                            items.append(
                                {
                                    "size": action.size,
                                    "status": "skipped",
                                    "reason": (
                                        f"cloud_wallet_post_not_visible_on_dexie:{visibility_error}"
                                    ),
                                    "offer_id": cloud_wallet_offer_id or None,
                                }
                            )
                            continue
                    executed_count += 1
                    items.append(
                        {
                            "size": action.size,
                            "status": "executed",
                            "reason": "cloud_wallet_post_success",
                            "offer_id": cloud_wallet_offer_id or None,
                        }
                    )
                else:
                    items.append(
                        {
                            "size": action.size,
                            "status": "skipped",
                            "reason": (
                                "cloud_wallet_post_failed:"
                                f"{str(cloud_wallet_post.get('error', 'unknown')).strip()}"
                            ),
                            "offer_id": None,
                        }
                    )
                continue

            built = _build_offer_for_action(
                market=market,
                action=action,
                xch_price_usd=xch_price_usd,
                network=app_network,
                keyring_yaml_path=keyring_yaml_path,
            )
            if built.get("status") != "executed":
                built_reason = str(built.get("reason", "offer_builder_skipped"))
                items.append(
                    {
                        "size": action.size,
                        "status": "skipped",
                        "reason": built_reason,
                        "offer_id": None,
                    }
                )
                continue

            remaining_ms = _cooldown_remaining_ms(_POST_COOLDOWN_UNTIL, cooldown_key)
            if remaining_ms > 0:
                items.append(
                    {
                        "size": action.size,
                        "status": "skipped",
                        "reason": f"post_cooldown_active:{remaining_ms}ms",
                        "offer_id": None,
                    }
                )
                continue

            offer_text = str(built["offer"])
            post_result, attempt_count, post_error = _post_offer_with_retry(
                publish_venue=publish_venue,
                offer_text=offer_text,
                dexie=dexie,
                splash=splash,
            )
            success = bool(post_result.get("success", False))
            offer_id_raw = post_result.get("id")
            offer_id = str(offer_id_raw).strip() if offer_id_raw is not None else ""
            if success and offer_id:
                executed_count += 1
                store.upsert_offer_state(
                    offer_id=offer_id,
                    market_id=market.market_id,
                    state=OfferLifecycleState.OPEN.value,
                    last_seen_status=0,
                )
                items.append(
                    {
                        "size": action.size,
                        "status": "executed",
                        "reason": f"{publish_venue}_post_success",
                        "offer_id": offer_id,
                        "attempts": attempt_count,
                    }
                )
            else:
                _set_cooldown(_POST_COOLDOWN_UNTIL, cooldown_key, cooldown_seconds)
                items.append(
                    {
                        "size": action.size,
                        "status": "skipped",
                        "reason": f"{publish_venue}_post_retry_exhausted:{post_error}",
                        "offer_id": offer_id or None,
                        "attempts": attempt_count,
                    }
                )
    return {
        "planned_count": sum(int(a.repeat) for a in strategy_actions),
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
    threshold_bps = _cancel_move_threshold_bps()
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
        print(json.dumps(payload))
        store.add_audit_event("low_inventory_alert", payload, market_id=market.market_id)
        send_pushover_alert(program, event)

    dexie_fetch_error: str | None = None
    try:
        offers = dexie.get_offers(market.base_asset, market.quote_asset)
        _log_market_decision(
            market.market_id,
            "dexie_offers_fetched",
            offered=market.base_asset,
            requested=market.quote_asset,
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
    if dexie_fetch_error is None:
        _update_market_coin_watchlist_from_dexie(
            market=market,
            offers=offers,
            store=store,
            clock=now,
        )
    for offer in offers:
        offer_id = str(offer.get("id", ""))
        if not offer_id:
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
            transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.MEMPOOL_SEEN)
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
            coin_count=len(wallet_coins),
            bucket_counts=bucket_counts,
        )
        store.add_audit_event(
            "inventory_bucket_scan",
            {
                "market_id": market.market_id,
                "source": "wallet_adapter",
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
    strategy_config = _strategy_config_from_market(market)
    active_offer_counts_by_size, offer_state_counts, active_unmapped_offer_ids = (
        _active_offer_counts_by_size(
            store=store,
            market_id=market.market_id,
            clock=now,
        )
    )
    _log_market_decision(
        market.market_id,
        "strategy_state_source",
        source="dexie_offer_coverage",
        active_offer_counts_by_size=active_offer_counts_by_size,
        state_counts=offer_state_counts,
        active_unmapped_offer_ids=active_unmapped_offer_ids,
    )
    strategy_actions = evaluate_market(
        state=_strategy_state_from_bucket_counts(
            active_offer_counts_by_size, xch_price_usd=xch_price_usd
        ),
        config=strategy_config,
        clock=now,
    )
    _log_market_decision(
        market.market_id,
        "strategy_evaluated",
        pair=strategy_config.pair,
        offer_counts=active_offer_counts_by_size,
        xch_price_usd=xch_price_usd,
        action_count=len(strategy_actions),
    )
    strategy_actions = _inject_reseed_action_if_no_active_offers(
        strategy_actions=strategy_actions,
        strategy_config=strategy_config,
        market=market,
        store=store,
        xch_price_usd=xch_price_usd,
        clock=now,
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
    buckets = [
        BucketSpec(
            size_base_units=e.size_base_units,
            target_count=e.target_count,
            split_buffer_count=e.split_buffer_count,
            combine_when_excess_factor=e.combine_when_excess_factor,
            current_count=int(bucket_counts.get(e.size_base_units, 0)),
        )
        for e in sell_ladder
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
            execution = wallet.execute_coin_ops(
                plans=executable_plans,
                dry_run=program.runtime_dry_run,
                key_id=signer_selection.key_id,
                network=program.app_network,
                market_id=market.market_id,
                asset_id=market.base_asset,
                receive_address=market.receive_address,
                onboarding_selection_path=state_dir / "key_onboarding.json",
                signer_fingerprint=signer_selection.fingerprint,
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
        cycle_error_count = 0
        strategy_planned_total = 0
        strategy_executed_total = 0
        cancel_triggered_count = 0
        cancel_planned_total = 0
        cancel_executed_total = 0
        dexie = DexieAdapter(program.dexie_api_base)
        splash = SplashAdapter(program.splash_api_base)
        wallet = WalletAdapter()
        price = PriceAdapter()
        previous_xch_price_usd = store.get_latest_xch_price_snapshot()
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
        for market in markets.markets:
            if not market.enabled:
                if _should_log_disabled_market(market_id=market.market_id):
                    _log_market_decision(market.market_id, "market_skipped", reason="disabled")
                continue
            _DISABLED_MARKET_NEXT_LOG_AT.pop(market.market_id, None)
            markets_processed += 1
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
            )
            cycle_error_count += mr.cycle_errors
            strategy_planned_total += mr.strategy_planned
            strategy_executed_total += mr.strategy_executed
            if mr.cancel_triggered:
                cancel_triggered_count += 1
            cancel_planned_total += mr.cancel_planned
            cancel_executed_total += mr.cancel_executed
        duration_ms = int((time.monotonic() - started_at) * 1000)
        store.add_audit_event(
            "daemon_cycle_summary",
            {
                "duration_ms": duration_ms,
                "markets_processed": markets_processed,
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
            )
            if _consume_reload_marker(state_dir):
                print(json.dumps({"event": "config_reloaded"}))
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
        default=".greenfloor/state",
        help="State directory used for reload marker and daemon-local state",
    )
    args = parser.parse_args()
    testnet_markets_path = (
        Path(args.testnet_markets_config) if str(args.testnet_markets_config).strip() else None
    )

    allowed_keys = {k.strip() for k in args.key_ids.split(",") if k.strip()} or None
    if args.once:
        program = load_program_config(Path(args.program_config))
        _initialize_daemon_file_logging(
            program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
        )
        _warn_if_log_level_auto_healed(program=program, program_path=Path(args.program_config))
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
            Path(args.state_dir),
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
            state_dir=Path(args.state_dir),
        )
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
