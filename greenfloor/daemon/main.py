"""Thin greenfloord entry: loop and --once both use in-process PyO3 cycle orchestration."""

from __future__ import annotations

import argparse
import logging
from pathlib import Path

from greenfloor.config.io import default_state_dir_path, load_program_config
from greenfloor.core.engine_bridge import import_engine, require_engine_method
from greenfloor.daemon.bootstrap import log_daemon_event
from greenfloor.daemon.cycle_runner import (
    new_coin_watchlist_cache,
    resolve_cycle_websocket_capture,
    run_loop,
    run_once,
)
from greenfloor.daemon.engine_logging import initialize_daemon_logging


def _engine():
    return import_engine()


def _acquire_daemon_instance_lock(*, state_dir: Path, mode: str):
    acquire = require_engine_method(
        _engine(),
        "acquire_daemon_instance_lock",
        missing="daemon instance lock",
    )
    return acquire(state_dir, mode)


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
        help="Run one evaluation cycle and exit (in-process Rust engine path)",
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
        if args.once:
            program = load_program_config(Path(args.program_config))
            initialize_daemon_logging(program=program, program_path=Path(args.program_config))
            use_websocket_capture = resolve_cycle_websocket_capture(
                program=program,
                loop_websocket_active=False,
            )
            with _acquire_daemon_instance_lock(state_dir=state_dir, mode="once"):
                exit_code = run_once(
                    program_path=Path(args.program_config),
                    markets_path=Path(args.markets_config),
                    testnet_markets_path=testnet_markets_path,
                    allowed_keys=allowed_keys,
                    db_path_override=args.state_db or None,
                    coinset_base_url=args.coinset_base_url,
                    state_dir=state_dir,
                    poll_coinset_mempool=not use_websocket_capture,
                    use_websocket_capture=use_websocket_capture,
                    coin_watchlist=new_coin_watchlist_cache(),
                )
            raise SystemExit(exit_code)

        with _acquire_daemon_instance_lock(
            state_dir=state_dir,
            mode="loop",
        ):
            exit_code = run_loop(
                program_path=Path(args.program_config),
                markets_path=Path(args.markets_config),
                testnet_markets_path=testnet_markets_path,
                allowed_keys=allowed_keys,
                db_path_override=args.state_db or None,
                coinset_base_url=args.coinset_base_url,
                state_dir=state_dir,
            )
    except Exception as exc:
        if "daemon_already_running" not in str(exc):
            raise
        try:
            program = load_program_config(Path(args.program_config))
            initialize_daemon_logging(program=program, program_path=Path(args.program_config))
        except Exception:
            pass
        log_daemon_event(
            level=logging.ERROR,
            payload={"event": "daemon_lock_conflict", "error": str(exc)},
        )
        raise SystemExit(3) from exc
    raise SystemExit(exit_code)


if __name__ == "__main__":
    main()
