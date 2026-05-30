"""Invoke the native greenfloor-engine CLI binary from Python manager commands."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any


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
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
    compact_json: bool,
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
        "--venue",
        publish_venue.strip(),
        "--dexie-base-url",
        dexie_base_url.strip(),
        "--splash-base-url",
        splash_base_url.strip(),
    ]
    if testnet_markets_path is not None:
        argv.extend(["--testnet-markets-config", str(testnet_markets_path)])
    if market_id:
        argv.extend(["--market-id", market_id.strip()])
    if pair:
        argv.extend(["--pair", pair.strip()])
    if not drop_only:
        argv.append("--allow-take")
    if claim_rewards:
        argv.append("--claim-rewards")
    if dry_run:
        argv.append("--dry-run")
    if compact_json:
        argv.append("--json")
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
    publish_venue: str,
    dexie_base_url: str,
    splash_base_url: str,
    drop_only: bool,
    claim_rewards: bool,
    dry_run: bool,
    compact_json: bool = False,
    run_fn: Callable[..., Any] | None = None,
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
    )
    runner = run_fn or subprocess.run
    completed = runner(argv, check=False)
    returncode = getattr(completed, "returncode", completed)
    if not isinstance(returncode, int):
        raise GreenfloorEngineBinaryError(
            f"unexpected subprocess return value from greenfloor-engine: {returncode!r}"
        )
    return returncode


def format_argv_for_display(argv: Sequence[str]) -> str:
    return " ".join(argv)
