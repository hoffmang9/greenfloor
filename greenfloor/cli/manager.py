from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import urllib.error
import urllib.request
from pathlib import Path

import yaml

from greenfloor.adapters.dexie import DexieAdapter
from greenfloor.adapters.splash import SplashAdapter
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


def _verify_offer_text_for_dexie(offer_text: str) -> str | None:
    try:
        import chia_wallet_sdk as sdk  # type: ignore
    except Exception as exc:
        return f"wallet_sdk_import_error:{exc}"
    try:
        verify_offer = getattr(sdk, "verify_offer", None)
        if not callable(verify_offer):
            return "wallet_sdk_verify_offer_unavailable"
        if not bool(verify_offer(offer_text)):
            return "wallet_sdk_offer_verify_false"
    except Exception as exc:
        return f"wallet_sdk_offer_verify_failed:{exc}"
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
            json.dumps(
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
            json.dumps(
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
        json.dumps(
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


def _build_offer_text_for_request(payload: dict) -> str:
    cmd_raw = os.getenv("GREENFLOOR_OFFER_BUILDER_CMD", "").strip()
    if cmd_raw:
        return _build_offer_text_via_subprocess(cmd_raw, payload)

    from greenfloor.cli.offer_builder_sdk import build_offer

    return build_offer(payload)


def _build_offer_text_via_subprocess(cmd_raw: str, payload: dict) -> str:
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
        raise RuntimeError(f"offer_builder_spawn_error:{exc}") from exc

    if completed.returncode != 0:
        err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
        raise RuntimeError(f"offer_builder_failed:{err}")

    try:
        body = json.loads(completed.stdout.strip() or "{}")
    except json.JSONDecodeError as exc:
        raise RuntimeError("offer_builder_invalid_json") from exc

    status = str(body.get("status", "skipped"))
    if status != "executed":
        raise RuntimeError(str(body.get("reason", "offer_builder_skipped")))

    offer = str(body.get("offer", "")).strip()
    if not offer:
        raise RuntimeError("offer_builder_missing_offer")
    if not offer.startswith("offer1"):
        raise RuntimeError("offer_builder_invalid_offer_prefix")
    return offer


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

    post_results: list[dict] = []
    built_offers_preview: list[dict[str, str]] = []
    dexie = DexieAdapter(dexie_base_url) if (not dry_run and publish_venue == "dexie") else None
    splash = SplashAdapter(splash_base_url) if (not dry_run and publish_venue == "splash") else None
    for _ in range(repeat):
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
            "expiry_value": 65,
            "quote_price_quote_per_base": float(quote_price),
            "base_unit_mojo_multiplier": int(pricing.get("base_unit_mojo_multiplier", 1000)),
            "quote_unit_mojo_multiplier": int(pricing.get("quote_unit_mojo_multiplier", 1000)),
            "dry_run": bool(dry_run),
            "key_id": market.signer_key_id,
            "keyring_yaml_path": keyring_yaml_path,
            "network": network,
            "asset_id": market.base_asset,
        }
        offer_text = _build_offer_text_for_request(payload)
        if dry_run:
            built_offers_preview.append(
                {
                    "offer_prefix": offer_text[:24],
                    "offer_length": str(len(offer_text)),
                }
            )
        else:
            if publish_venue == "dexie":
                assert dexie is not None
                verify_error = _verify_offer_text_for_dexie(offer_text)
                if verify_error:
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
                post_results.append({"venue": "dexie", "result": result})
            else:
                assert splash is not None
                result = splash.post_offer(offer_text)
                post_results.append({"venue": "splash", "result": result})

    print(
        json.dumps(
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
                "built_offers_preview": built_offers_preview,
                "results": post_results,
            }
        )
    )
    return 0


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
        json.dumps(
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
    print(json.dumps(result))
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
                    if status is None:
                        next_state = "unknown_orphaned"
                        reason = "missing_status"
                    else:
                        next_state = _reconciled_state_from_dexie_status(
                            status=status,
                            current_state=current_state,
                        )
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
                }
            )
        print(
            json.dumps(
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
        json.dumps(
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


# ---------------------------------------------------------------------------
# CLI entry point
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="GreenFloor manager CLI")
    parser.add_argument("--program-config", default=_default_program_config_path())
    parser.add_argument("--markets-config", default=_default_markets_config_path())
    parser.add_argument("--state-db", default="")

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

    p_bootstrap = sub.add_parser("bootstrap-home")
    p_bootstrap.add_argument("--home-dir", default="~/.greenfloor")
    p_bootstrap.add_argument("--program-template", default="config/program.yaml")
    p_bootstrap.add_argument("--markets-template", default="config/markets.yaml")
    p_bootstrap.add_argument("--force", action="store_true")

    args = parser.parse_args()
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
    elif args.command == "bootstrap-home":
        code = _bootstrap_home(
            home_dir=Path(args.home_dir),
            program_template=Path(args.program_template),
            markets_template=Path(args.markets_template),
            force=bool(args.force),
        )
    else:
        raise ValueError(f"unsupported command: {args.command}")
    raise SystemExit(code)


if __name__ == "__main__":
    main()
