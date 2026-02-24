from __future__ import annotations

import argparse
import datetime as dt
import importlib
import json
import os
import time
import urllib.error
import urllib.request
from pathlib import Path

import yaml

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter
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


_FEE_ADVICE_CACHE: dict[str, int | float] = {}


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


def _resolve_taker_or_coin_operation_fee(*, network: str) -> tuple[int, str]:
    if os.getenv("GREENFLOOR_COINSET_ADVISED_FEE_MOJOS", "").strip():
        value = int(os.getenv("GREENFLOOR_COINSET_ADVISED_FEE_MOJOS", "0").strip())
        return value, "env_override"
    now = time.time()
    ttl_seconds = int(os.getenv("GREENFLOOR_COINSET_FEE_CACHE_TTL_SECONDS", "600"))
    cached_value = _FEE_ADVICE_CACHE.get("fee_mojos")
    cached_at = _FEE_ADVICE_CACHE.get("cached_at_epoch")
    if isinstance(cached_value, int) and isinstance(cached_at, float):
        if now - cached_at <= max(1, ttl_seconds):
            return cached_value, "cached_last_good"

    max_attempts = int(os.getenv("GREENFLOOR_COINSET_FEE_MAX_ATTEMPTS", "4"))
    coinset = CoinsetAdapter(None, network=network)
    for attempt in range(max_attempts):
        advised = coinset.get_conservative_fee_estimate()
        if advised is not None:
            _FEE_ADVICE_CACHE["fee_mojos"] = advised
            _FEE_ADVICE_CACHE["cached_at_epoch"] = now
            return advised, "coinset_conservative"
        if attempt < max_attempts - 1:
            sleep_seconds = min(8.0, 0.5 * (2**attempt))
            time.sleep(sleep_seconds)

    if isinstance(cached_value, int):
        return cached_value, "cached_last_good_stale"
    raise RuntimeError(
        "fee_advice_unavailable:coinset unavailable; retry later or set GREENFLOOR_COINSET_ADVISED_FEE_MOJOS"
    )


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
    sleep_seconds = 2.0
    while True:
        status_payload = wallet.get_signature_request(signature_request_id=signature_request_id)
        status = str(status_payload.get("status", "")).strip().upper()
        if status and status != "UNSIGNED":
            return status, events

        elapsed = int(time.monotonic() - start)
        if elapsed >= timeout_seconds:
            raise RuntimeError("signature_request_timeout_waiting_for_signature")
        if elapsed >= next_warning:
            events.append(
                {
                    "event": "signature_wait_warning",
                    "elapsed_seconds": str(elapsed),
                    "message": "still_waiting_on_user_signature",
                }
            )
            next_warning += warning_interval_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


def _wait_for_mempool_then_confirmation(
    *,
    wallet: CloudWalletAdapter,
    initial_coin_ids: set[str],
    mempool_warning_seconds: int,
    confirmation_warning_seconds: int,
) -> list[dict[str, str]]:
    events: list[dict[str, str]] = []
    start = time.monotonic()
    seen_pending = False
    sleep_seconds = 2.0
    while True:
        coins = wallet.list_coins(include_pending=True)
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
        elapsed = int(time.monotonic() - start)
        if pending and not seen_pending:
            seen_pending = True
            sample = str(pending[0].get("name", pending[0].get("id", ""))).strip()
            events.append(
                {
                    "event": "in_mempool",
                    "coinset_url": f"https://coinset.org/coin/{sample}",
                    "elapsed_seconds": str(elapsed),
                }
            )
        if confirmed:
            return events

        if not seen_pending and elapsed >= mempool_warning_seconds:
            events.append(
                {
                    "event": "mempool_wait_warning",
                    "elapsed_seconds": str(elapsed),
                }
            )
            mempool_warning_seconds += mempool_warning_seconds
        if seen_pending and elapsed >= confirmation_warning_seconds:
            events.append(
                {
                    "event": "confirmation_wait_warning",
                    "elapsed_seconds": str(elapsed),
                }
            )
            confirmation_warning_seconds += confirmation_warning_seconds
        time.sleep(sleep_seconds)
        sleep_seconds = min(20.0, sleep_seconds * 1.5)


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
) -> int:
    wallet = _new_cloud_wallet_adapter(program)
    post_results: list[dict] = []
    publish_failures = 0
    dexie = DexieAdapter(dexie_base_url) if publish_venue == "dexie" else None
    splash = SplashAdapter(splash_base_url) if publish_venue == "splash" else None

    for _ in range(repeat):
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

        offered = [{"assetId": str(market.base_asset), "amount": offer_amount}]
        requested = [{"assetId": str(market.quote_asset), "amount": request_amount}]
        expires_at = (dt.datetime.now(dt.UTC) + dt.timedelta(minutes=65)).isoformat()
        create_result = wallet.create_offer(
            offered=offered,
            requested=requested,
            fee=0,
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

        wallet_payload = wallet.get_wallet()
        offers = wallet_payload.get("offers", [])
        offer_text = ""
        for offer in offers:
            bech32 = str(offer.get("bech32", "")).strip()
            if bech32.startswith("offer1"):
                offer_text = bech32
                break
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
        json.dumps(
            {
                "market_id": market.market_id,
                "pair": f"{market.base_asset}:{market.quote_asset}",
                "network": program.app_network,
                "size_base_units": size_base_units,
                "repeat": repeat,
                "publish_venue": publish_venue,
                "dexie_base_url": dexie_base_url,
                "splash_base_url": splash_base_url if publish_venue == "splash" else None,
                "drop_only": drop_only,
                "claim_rewards": claim_rewards,
                "dry_run": False,
                "publish_attempts": len(post_results),
                "publish_failures": publish_failures,
                "built_offers_preview": [],
                "results": post_results,
                "offer_fee_mojos": 0,
            }
        )
    )
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
    if cloud_wallet_configured and not dry_run:
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
            "expiry_value": 65,
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
    asset_filter = asset.strip() if asset else ""
    if asset_filter.lower() in {"xch", "txch"}:
        asset_filter = None
    coins = wallet.list_coins(asset_id=asset_filter or None, include_pending=True)
    items = []
    for coin in coins:
        coin_state = str(coin.get("state", "")).strip().upper()
        pending = coin_state in {"PENDING", "MEMPOOL"}
        spendable = coin_state not in {"SPENT"} and not pending
        asset_payload = coin.get("asset") if isinstance(coin.get("asset"), dict) else {}
        items.append(
            {
                "coin_id": str(coin.get("name", coin.get("id", ""))).strip(),
                "amount": int(coin.get("amount", 0)),
                "state": coin_state or "UNKNOWN",
                "pending": pending,
                "spendable": spendable,
                "asset": str(asset_payload.get("id", "xch")).strip(),
            }
        )
    print(
        json.dumps(
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
) -> int:
    if amount_per_coin <= 0:
        raise ValueError("amount_per_coin must be positive")
    if number_of_coins <= 0:
        raise ValueError("number_of_coins must be positive")
    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)
    market = _resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    wallet = _new_cloud_wallet_adapter(program)
    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in wallet.list_coins(include_pending=True)
    }
    fee_mojos, fee_source = _resolve_taker_or_coin_operation_fee(network=network)
    split_result = wallet.split_coins(
        coin_ids=coin_ids,
        amount_per_coin=amount_per_coin,
        number_of_coins=number_of_coins,
        fee=fee_mojos,
    )
    signature_request_id = split_result["signature_request_id"]
    if not signature_request_id:
        raise RuntimeError("coin_split_failed:missing_signature_request_id")

    wait_events: list[dict[str, str]] = []
    final_signature_state = split_result.get("status", "UNKNOWN")
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
                initial_coin_ids=existing_coin_ids,
                mempool_warning_seconds=5 * 60,
                confirmation_warning_seconds=15 * 60,
            )
        )
    print(
        json.dumps(
            {
                "market_id": market.market_id,
                "pair": f"{market.base_symbol}:{market.quote_asset}",
                "vault_id": wallet.vault_id,
                "signature_request_id": signature_request_id,
                "signature_state": final_signature_state,
                "waited": not no_wait,
                "wait_events": wait_events,
                "fee_mojos": fee_mojos,
                "fee_source": fee_source,
            }
        )
    )
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
    no_wait: bool,
) -> int:
    if number_of_coins <= 1:
        raise ValueError("number_of_coins must be > 1")
    program = load_program_config(program_path)
    markets = load_markets_config(markets_path)
    market = _resolve_market_for_build(
        markets,
        market_id=market_id,
        pair=pair,
        network=network,
    )
    wallet = _new_cloud_wallet_adapter(program)
    existing_coin_ids = {
        str(c.get("id", "")).strip() for c in wallet.list_coins(include_pending=True)
    }
    fee_mojos, fee_source = _resolve_taker_or_coin_operation_fee(network=network)
    combine_result = wallet.combine_coins(
        number_of_coins=number_of_coins,
        fee=fee_mojos,
        asset_id=asset_id,
        largest_first=True,
    )
    signature_request_id = combine_result["signature_request_id"]
    if not signature_request_id:
        raise RuntimeError("coin_combine_failed:missing_signature_request_id")

    wait_events: list[dict[str, str]] = []
    final_signature_state = combine_result.get("status", "UNKNOWN")
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
                initial_coin_ids=existing_coin_ids,
                mempool_warning_seconds=5 * 60,
                confirmation_warning_seconds=15 * 60,
            )
        )
    print(
        json.dumps(
            {
                "market_id": market.market_id,
                "pair": f"{market.base_symbol}:{market.quote_asset}",
                "vault_id": wallet.vault_id,
                "signature_request_id": signature_request_id,
                "signature_state": final_signature_state,
                "waited": not no_wait,
                "wait_events": wait_events,
                "fee_mojos": fee_mojos,
                "fee_source": fee_source,
            }
        )
    )
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
    p_coin_split.add_argument("--amount-per-coin", required=True, type=int)
    p_coin_split.add_argument("--number-of-coins", required=True, type=int)
    p_coin_split.add_argument("--no-wait", action="store_true")

    p_coin_combine = sub.add_parser("coin-combine")
    combine_market_group = p_coin_combine.add_mutually_exclusive_group(required=True)
    combine_market_group.add_argument("--market-id", default="")
    combine_market_group.add_argument("--pair", default="")
    p_coin_combine.add_argument(
        "--network", default="mainnet", choices=["mainnet", "testnet", "testnet11"]
    )
    p_coin_combine.add_argument("--number-of-coins", required=True, type=int)
    p_coin_combine.add_argument("--asset-id", default="")
    p_coin_combine.add_argument("--no-wait", action="store_true")

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
        )
    elif args.command == "coin-combine":
        code = _coin_combine(
            program_path=Path(args.program_config),
            markets_path=Path(args.markets_config),
            network=args.network,
            market_id=args.market_id or None,
            pair=args.pair or None,
            number_of_coins=int(args.number_of_coins),
            asset_id=args.asset_id or None,
            no_wait=bool(args.no_wait),
        )
    else:
        raise ValueError(f"unsupported command: {args.command}")
    raise SystemExit(code)


if __name__ == "__main__":
    main()
