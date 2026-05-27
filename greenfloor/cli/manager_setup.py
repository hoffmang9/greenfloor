"""CLI setup, validation, and health-check commands."""

from __future__ import annotations

import os
from pathlib import Path

import yaml

from greenfloor.config.io import (
    load_markets_config_with_optional_overlay,
    load_program_config,
    load_yaml,
    resolve_state_db_path,
    write_yaml,
)
from greenfloor.keys.router import resolve_market_key
from greenfloor.logging_setup import (
    ALLOWED_LOG_LEVELS,
    normalize_log_level_name,
)
from greenfloor.runtime.cloud_wallet.adapter import format_json_output
from greenfloor.storage.sqlite import SqliteStore


def validate_config(
    program_path: Path, markets_path: Path, testnet_markets_path: Path | None = None
) -> int:
    program = load_program_config(program_path)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )
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


def set_log_level(*, program_path: Path, log_level: str) -> int:
    level = normalize_log_level_name(log_level)
    if level != str(log_level).strip().upper():
        raise ValueError(f"log level must be one of: {', '.join(sorted(ALLOWED_LOG_LEVELS))}")
    raw = load_yaml(program_path)
    app = raw.get("app")
    if app is None:
        app = {}
        raw["app"] = app
    if not isinstance(app, dict):
        raise ValueError("program config field 'app' must be a mapping")
    prior_level = str(app.get("log_level", "")).strip().upper() or "INFO"
    app["log_level"] = level
    write_yaml(program_path, raw)
    print(
        format_json_output(
            {
                "updated": True,
                "program_config": str(program_path),
                "previous_log_level": prior_level,
                "log_level": level,
            }
        )
    )
    return 0


def bootstrap_home(
    *,
    home_dir: Path,
    program_template: Path,
    markets_template: Path,
    cats_template: Path | None,
    testnet_markets_template: Path | None,
    seed_testnet_markets: bool,
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
    seeded_cats = config_dir / "cats.yaml"
    seeded_testnet_markets = config_dir / "testnet-markets.yaml"

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

    wrote_cats = False
    if cats_template is not None and (force or not seeded_cats.exists()):
        cats_data = load_yaml(cats_template)
        seeded_cats.write_text(
            yaml.safe_dump(cats_data, sort_keys=False),
            encoding="utf-8",
        )
        wrote_cats = True

    wrote_testnet_markets = False
    if (
        seed_testnet_markets
        and testnet_markets_template is not None
        and (force or not seeded_testnet_markets.exists())
    ):
        testnet_markets_data = load_yaml(testnet_markets_template)
        seeded_testnet_markets.write_text(
            yaml.safe_dump(testnet_markets_data, sort_keys=False),
            encoding="utf-8",
        )
        wrote_testnet_markets = True

    db_path = (db_dir / "greenfloor.sqlite").resolve()
    store = SqliteStore(db_path)
    try:
        store.add_audit_event(
            "home_bootstrap",
            {
                "home_dir": str(home),
                "program_config": str(seeded_program),
                "markets_config": str(seeded_markets),
                "cats_config": str(seeded_cats),
                "testnet_markets_config": str(seeded_testnet_markets),
                "force": bool(force),
            },
        )
    finally:
        store.close()

    print(
        format_json_output(
            {
                "bootstrapped": True,
                "home_dir": str(home),
                "program_config": str(seeded_program),
                "markets_config": str(seeded_markets),
                "cats_config": str(seeded_cats),
                "testnet_markets_config": (
                    str(seeded_testnet_markets) if bool(seed_testnet_markets) else ""
                ),
                "state_db": str(db_path),
                "state_dir": str(state_dir),
                "logs_dir": str(logs_dir),
                "wrote_program_config": wrote_program,
                "wrote_markets_config": wrote_markets,
                "wrote_cats_config": wrote_cats,
                "wrote_testnet_markets_config": wrote_testnet_markets,
            }
        )
    )
    return 0


def doctor(
    program_path: Path,
    markets_path: Path,
    state_db: str | None,
    testnet_markets_path: Path | None = None,
) -> int:
    program = load_program_config(program_path)
    markets = load_markets_config_with_optional_overlay(
        path=markets_path,
        overlay_path=testnet_markets_path,
    )

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

    db_path = resolve_state_db_path(
        program_config_path=program_path,
        explicit_db_path=state_db,
    )
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
    print(format_json_output(result))
    return 0 if not problems else 2
