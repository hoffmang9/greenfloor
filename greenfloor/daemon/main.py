"""Thin greenfloord entry: --once delegates to native greenfloor-engine."""

from __future__ import annotations

import argparse
import contextlib
import fcntl
import json
import logging
import os
from datetime import UTC, datetime
from pathlib import Path

from greenfloor.cli.engine_binary import run_daemon_once_via_engine
from greenfloor.config.io import default_state_dir_path, load_program_config
from greenfloor.daemon.bootstrap import (
    initialize_daemon_file_logging,
    log_daemon_event,
    warn_if_daemon_log_level_auto_healed,
)
from greenfloor.daemon.cycle_runner import run_loop

_DAEMON_INSTANCE_LOCK_FILENAME = "daemon.lock"


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
        help="Run one evaluation cycle and exit (native Rust engine path)",
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
            exit_code = run_daemon_once_via_engine(
                program_path=Path(args.program_config),
                markets_path=Path(args.markets_config),
                testnet_markets_path=testnet_markets_path,
                key_ids=args.key_ids or None,
                state_db=args.state_db or None,
                coinset_base_url=args.coinset_base_url,
                state_dir=state_dir,
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
    except RuntimeError as exc:
        try:
            program = load_program_config(Path(args.program_config))
            initialize_daemon_file_logging(
                program.home_dir, log_level=getattr(program, "app_log_level", "INFO")
            )
            warn_if_daemon_log_level_auto_healed(
                program=program, program_path=Path(args.program_config)
            )
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
