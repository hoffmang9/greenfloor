from __future__ import annotations

import argparse
import asyncio
import json
import os
import shlex
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.price import PriceAdapter
from greenfloor.adapters.splash import SplashAdapter
from greenfloor.adapters.wallet import WalletAdapter
from greenfloor.config.io import load_markets_config, load_program_config
from greenfloor.core.coin_ops import BucketSpec, plan_coin_ops
from greenfloor.core.fee_budget import partition_plans_by_budget, projected_coin_ops_fee_mojos
from greenfloor.core.inventory import compute_bucket_counts_from_coins
from greenfloor.core.notifications import AlertState, evaluate_low_inventory_alert, utcnow
from greenfloor.core.offer_lifecycle import OfferLifecycleState, OfferSignal, apply_offer_signal
from greenfloor.core.strategy import MarketState, StrategyConfig, evaluate_market
from greenfloor.daemon.reload import consume_reload_marker
from greenfloor.daemon.webhook import start_coinset_webhook_server
from greenfloor.keys.router import resolve_market_key
from greenfloor.notify.pushover import send_pushover_alert
from greenfloor.storage.sqlite import SqliteStore, StoredAlertState

_DEFAULT_CANCEL_MOVE_THRESHOLD_BPS = 500
_POST_COOLDOWN_UNTIL: dict[str, float] = {}
_CANCEL_COOLDOWN_UNTIL: dict[str, float] = {}


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


def _post_offer_with_retry(
    *,
    publish_venue: str,
    offer_text: str,
    dexie: DexieAdapter,
    splash: SplashAdapter | None,
) -> tuple[dict[str, Any], int, str]:
    attempts_max, backoff_ms, _ = _post_retry_config()
    last_error = f"{publish_venue}_post_failed"
    for attempt in range(1, attempts_max + 1):
        try:
            if publish_venue == "splash":
                if splash is None:
                    return (
                        {"success": False, "error": "splash_not_configured"},
                        attempt,
                        "splash_not_configured",
                    )
                result = splash.post_offer(offer_text)
            else:
                result = dexie.post_offer(offer_text)
        except Exception as exc:
            result = {"success": False, "error": f"{publish_venue}_post_error:{exc}"}
        if bool(result.get("success", False)) and str(result.get("id", "")).strip():
            return result, attempt, ""
        last_error = str(result.get("error", f"{publish_venue}_post_failed"))
        if attempt < attempts_max and backoff_ms > 0:
            time.sleep((backoff_ms * (2 ** (attempt - 1))) / 1000.0)
    return {"success": False, "error": last_error}, attempts_max, last_error


def _cancel_offer_with_retry(
    *,
    dexie: DexieAdapter,
    offer_id: str,
) -> tuple[dict[str, Any], int, str]:
    attempts_max, backoff_ms, _ = _cancel_retry_config()
    last_error = "cancel_offer_failed"
    for attempt in range(1, attempts_max + 1):
        try:
            result = dexie.cancel_offer(offer_id)
        except Exception as exc:
            result = {"success": False, "error": f"cancel_offer_error:{exc}"}
        if bool(result.get("success", False)):
            return result, attempt, ""
        last_error = str(result.get("error", "cancel_offer_failed"))
        if attempt < attempts_max and backoff_ms > 0:
            time.sleep((backoff_ms * (2 ** (attempt - 1))) / 1000.0)
    return {"success": False, "error": last_error}, attempts_max, last_error


def _normalize_strategy_pair(quote_asset: str) -> str:
    lowered = quote_asset.strip().lower()
    if lowered == "xch":
        return "xch"
    if "usdc" in lowered:
        return "usdc"
    return lowered


def _strategy_config_from_market(market) -> StrategyConfig:
    sell_ladder = market.ladders.get("sell", [])
    targets_by_size = {int(e.size_base_units): int(e.target_count) for e in sell_ladder}
    pricing = dict(getattr(market, "pricing", {}) or {})

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


def _build_offer_for_action(
    *,
    market,
    action,
    xch_price_usd: float | None,
) -> dict[str, Any]:
    cmd_raw = os.getenv("GREENFLOOR_OFFER_BUILDER_CMD", "").strip()
    if not cmd_raw:
        cmd_raw = f"{sys.executable} -m greenfloor.cli.offer_builder_sdk"

    payload = {
        "market_id": market.market_id,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": market.quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "size_base_units": int(action.size),
        "pair": action.pair,
        "reason": action.reason,
        "xch_price_usd": xch_price_usd,
        "target_spread_bps": action.target_spread_bps,
        "expiry_unit": action.expiry_unit,
        "expiry_value": int(action.expiry_value),
    }
    try:
        completed = subprocess.run(
            shlex.split(cmd_raw),
            input=json.dumps(payload),
            capture_output=True,
            check=False,
            text=True,
            timeout=120,
        )
    except Exception as exc:
        return {"status": "skipped", "reason": f"offer_builder_spawn_error:{exc}", "offer": None}

    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        return {"status": "skipped", "reason": f"offer_builder_failed:{err}", "offer": None}

    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError:
        return {"status": "skipped", "reason": "offer_builder_invalid_json", "offer": None}

    offer = str(body.get("offer", "")).strip()
    if not offer:
        return {"status": "skipped", "reason": "offer_builder_missing_offer", "offer": None}
    return {
        "status": str(body.get("status", "executed")),
        "reason": str(body.get("reason", "offer_builder_success")),
        "offer": offer,
    }


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
) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    executed_count = 0
    _, _, cooldown_seconds = _post_retry_config()
    cooldown_key = f"{publish_venue}:{market.market_id}"
    for action in strategy_actions:
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

            built = _build_offer_for_action(
                market=market,
                action=action,
                xch_price_usd=xch_price_usd,
            )
            if built.get("status") != "executed":
                items.append(
                    {
                        "size": action.size,
                        "status": "skipped",
                        "reason": str(built.get("reason", "offer_builder_skipped")),
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
    pricing = dict(getattr(market, "pricing", {}) or {})
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


def run_once(
    program_path: Path,
    markets_path: Path,
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)
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
        try:
            coinset = CoinsetAdapter(coinset_base_url)
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
                continue
            markets_processed += 1
            signer_selection = resolve_market_key(
                market,
                allowed_keys,
                signer_key_registry=program.signer_key_registry,
                required_network=program.app_network,
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

            # Offer lifecycle transitions from live Dexie status snapshots.
            try:
                offers = dexie.get_offers(market.base_asset, market.quote_asset)
            except Exception as exc:  # pragma: no cover - network dependent
                cycle_error_count += 1
                store.add_audit_event(
                    "dexie_offers_error",
                    {"market_id": market.market_id, "error": str(exc)},
                    market_id=market.market_id,
                )
                offers = []
            for offer in offers:
                offer_id = str(offer.get("id", ""))
                if not offer_id:
                    continue
                status = int(offer.get("status", -1))
                if status == 4:
                    transition = apply_offer_signal(
                        OfferLifecycleState.OPEN, OfferSignal.TX_CONFIRMED
                    )
                elif status == 6:
                    transition = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRED)
                else:
                    transition = apply_offer_signal(
                        OfferLifecycleState.OPEN, OfferSignal.MEMPOOL_SEEN
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
                cancel_triggered_count += 1
            cancel_planned_total += int(cancel_policy.get("planned_count", 0))
            cancel_executed_total += int(cancel_policy.get("executed_count", 0))
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

            # Ladder-aware coin ops planning from market config.
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
            strategy_actions = evaluate_market(
                state=_strategy_state_from_bucket_counts(
                    bucket_counts,
                    xch_price_usd=xch_price_usd,
                ),
                config=_strategy_config_from_market(market),
                clock=now,
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
            )
            strategy_planned_total += int(offer_execution["planned_count"])
            strategy_executed_total += int(offer_execution["executed_count"])
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
                else:
                    execution = {
                        "dry_run": program.runtime_dry_run,
                        "planned_count": 0,
                        "executed_count": 0,
                        "status": "skipped_fee_budget",
                        "items": [],
                    }
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
                            str(item.get("operation_id"))
                            if item.get("operation_id") is not None
                            else None
                        ),
                    )
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
    allowed_keys: set[str] | None,
    db_path_override: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> int:
    program = load_program_config(program_path)
    webhook_server = None
    store_for_hook: SqliteStore | None = None
    if program.tx_block_webhook_enabled:
        db_path = _resolve_db_path(program.home_dir, db_path_override)
        store_for_hook = SqliteStore(db_path)

        def _extract_tx_ids(payload: dict) -> list[str]:
            if "tx_ids" in payload and isinstance(payload["tx_ids"], list):
                return [str(x) for x in payload["tx_ids"]]
            if "tx_id" in payload:
                return [str(payload["tx_id"])]
            return []

        def _on_webhook_event(payload: dict) -> None:
            store_for_hook.add_audit_event("coinset_tx_block_webhook", payload)
            tx_ids = _extract_tx_ids(payload)
            if tx_ids:
                confirmed = store_for_hook.confirm_tx_ids(tx_ids)
                store_for_hook.add_audit_event(
                    "tx_block_confirmed",
                    {"tx_ids": tx_ids, "confirmed_count": confirmed},
                )

        webhook_server, _thread = start_coinset_webhook_server(
            program.tx_block_webhook_listen_addr,
            _on_webhook_event,
        )

    try:
        while True:
            run_once(
                program_path=program_path,
                markets_path=markets_path,
                allowed_keys=allowed_keys,
                db_path_override=db_path_override,
                coinset_base_url=coinset_base_url,
                state_dir=state_dir,
            )
            if consume_reload_marker(state_dir):
                print(json.dumps({"event": "config_reloaded"}))
            time.sleep(max(1, program.runtime_loop_interval_seconds))
    except KeyboardInterrupt:
        return 0
    finally:
        if webhook_server is not None:
            webhook_server.shutdown()
            webhook_server.server_close()
        if store_for_hook is not None:
            store_for_hook.close()


def main() -> None:
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
        default="https://coinset.org",
        help="Coinset API base URL",
    )
    parser.add_argument(
        "--state-dir",
        default=".greenfloor/state",
        help="State directory used for reload marker and daemon-local state",
    )
    args = parser.parse_args()

    allowed_keys = {k.strip() for k in args.key_ids.split(",") if k.strip()} or None
    if args.once:
        exit_code = run_once(
            Path(args.program_config),
            Path(args.markets_config),
            allowed_keys,
            args.state_db or None,
            args.coinset_base_url,
            Path(args.state_dir),
        )
    else:
        exit_code = _run_loop(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            allowed_keys=allowed_keys,
            db_path_override=args.state_db or None,
            coinset_base_url=args.coinset_base_url,
            state_dir=Path(args.state_dir),
        )
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
