"""Invoke the native greenfloor-engine CLI binary from Python manager commands."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path


class GreenfloorEngineBinaryError(RuntimeError):
    """Raised when the greenfloor-engine binary cannot be located or executed."""


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def resolve_greenfloor_engine_binary() -> Path:
    override = os.environ.get("GREENFLOOR_ENGINE_BIN", "").strip()
    if override:
        path = Path(override).expanduser()
        if not path.is_file():
            raise GreenfloorEngineBinaryError(
                f"GREENFLOOR_ENGINE_BIN is not an executable file: {path}"
            )
        return path

    discovered = shutil.which("greenfloor-engine")
    if discovered:
        return Path(discovered)

    root = repo_root()
    for relative in (
        Path("target/release/greenfloor-engine"),
        Path("target/debug/greenfloor-engine"),
        Path("greenfloor-engine/target/release/greenfloor-engine"),
        Path("greenfloor-engine/target/debug/greenfloor-engine"),
    ):
        candidate = root / relative
        if candidate.is_file():
            return candidate

    raise GreenfloorEngineBinaryError(
        "greenfloor-engine binary not found; build with "
        "'cargo build --manifest-path greenfloor-engine/Cargo.toml' or set GREENFLOOR_ENGINE_BIN"
    )


def build_and_post_offer_argv(
    *,
    binary: Path,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    network: str,
    market_id: str | None,
    pair: str | None,
    size_base_units: int,
    repeat: int,
    publish_venue: str | None,
    dexie_base_url: str | None,
    splash_base_url: str | None,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
    compact_json: bool,
    persist_results: bool,
) -> list[str]:
    argv: list[str] = [
        str(binary),
        "build-and-post-offer",
        "--program-config",
        str(program_path),
        "--markets-config",
        str(markets_path),
        "--network",
        network.strip(),
        "--size-base-units",
        str(int(size_base_units)),
        "--repeat",
        str(int(repeat)),
    ]
    if testnet_markets_path is not None:
        argv.extend(["--testnet-markets-config", str(testnet_markets_path)])
    if market_id:
        argv.extend(["--market-id", market_id.strip()])
    if pair:
        argv.extend(["--pair", pair.strip()])
    if publish_venue and publish_venue.strip():
        argv.extend(["--venue", publish_venue.strip()])
    if dexie_base_url and dexie_base_url.strip():
        argv.extend(["--dexie-base-url", dexie_base_url.strip()])
    if splash_base_url and splash_base_url.strip():
        argv.extend(["--splash-base-url", splash_base_url.strip()])
    if not drop_only:
        argv.append("--allow-take")
    if claim_rewards:
        argv.append("--claim-rewards")
    if dry_run:
        argv.append("--dry-run")
    if compact_json:
        argv.append("--json")
    if not persist_results:
        argv.append("--no-persist-results")
    return argv


def run_build_and_post_offer_via_engine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    network: str,
    market_id: str | None,
    pair: str | None,
    size_base_units: int,
    repeat: int,
    publish_venue: str | None,
    dexie_base_url: str | None,
    splash_base_url: str | None,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
    compact_json: bool = False,
    persist_results: bool = True,
    run_fn: Callable[..., object] | None = None,
) -> int:
    binary = resolve_greenfloor_engine_binary()
    argv = build_and_post_offer_argv(
        binary=binary,
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        network=network,
        market_id=market_id,
        pair=pair,
        size_base_units=size_base_units,
        repeat=repeat,
        publish_venue=publish_venue,
        dexie_base_url=dexie_base_url,
        splash_base_url=splash_base_url,
        drop_only=drop_only,
        claim_rewards=claim_rewards,
        dry_run=dry_run,
        compact_json=compact_json,
        persist_results=persist_results,
    )
    runner = run_fn or subprocess.run
    completed = runner(argv, check=False)
    returncode = getattr(completed, "returncode", completed)
    if not isinstance(returncode, int):
        raise GreenfloorEngineBinaryError(
            f"unexpected subprocess return value from greenfloor-engine: {returncode!r}"
        )
    return returncode


def daemon_run_once_argv(
    *,
    binary: Path,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None,
    key_ids: str | None,
    state_db: str | None,
    coinset_base_url: str,
    state_dir: Path,
) -> list[str]:
    argv: list[str] = [
        str(binary),
        "daemon",
        "run-once",
        "--program-config",
        str(program_path),
        "--markets-config",
        str(markets_path),
        "--coinset-base-url",
        coinset_base_url.strip(),
        "--state-dir",
        str(state_dir),
    ]
    if testnet_markets_path is not None:
        argv.extend(["--testnet-markets-config", str(testnet_markets_path)])
    if key_ids and key_ids.strip():
        argv.extend(["--key-ids", key_ids.strip()])
    if state_db and state_db.strip():
        argv.extend(["--state-db", state_db.strip()])
    return argv


def run_daemon_once_via_engine(
    *,
    program_path: Path,
    markets_path: Path,
    testnet_markets_path: Path | None = None,
    key_ids: str | None = None,
    state_db: str | None = None,
    coinset_base_url: str = "https://api.coinset.org",
    state_dir: Path,
    run_fn: Callable[..., object] | None = None,
) -> int:
    binary = resolve_greenfloor_engine_binary()
    argv = daemon_run_once_argv(
        binary=binary,
        program_path=program_path,
        markets_path=markets_path,
        testnet_markets_path=testnet_markets_path,
        key_ids=key_ids,
        state_db=state_db,
        coinset_base_url=coinset_base_url,
        state_dir=state_dir,
    )
    runner = run_fn or subprocess.run
    completed = runner(argv, check=False)
    returncode = getattr(completed, "returncode", completed)
    if not isinstance(returncode, int):
        raise GreenfloorEngineBinaryError(
            f"unexpected subprocess return value from greenfloor-engine: {returncode!r}"
        )
    return returncode
