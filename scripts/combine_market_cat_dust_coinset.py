#!/usr/bin/env python3
"""Combine sub-unit CAT dust for assets referenced by enabled markets (Coinset path).

Scans the vault via ``list_vault_coins_coinset.py`` (launcher + nonce scan), selects
unspent CAT coins with amount strictly below the dust threshold (default 1000 mojos =
one CAT unit), then runs ``combine_coinset_direct.py`` in batches.

Requires the same Cloud Wallet + KMS + keyring inputs as the underlying scripts; read
from ``program.yaml`` and ``markets.yaml`` (optional testnet overlay), not from flags
except overrides documented in ``--help``.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from greenfloor.config.io import (
    default_cats_config_path,
    load_markets_config_with_optional_overlay,
    load_program_config,
    load_yaml,
)
from greenfloor.hex_utils import normalize_hex_id


@dataclass(frozen=True, slots=True)
class CatDustJob:
    cat_asset_id: str
    signer_key_id: str
    receive_address: str
    market_ids: tuple[str, ...] = field(default_factory=tuple)


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _load_symbol_to_cat_asset_id(cats_path: Path) -> dict[str, str]:
    raw = load_yaml(cats_path)
    cats = raw.get("cats")
    if not isinstance(cats, list):
        return {}
    out: dict[str, str] = {}
    for row in cats:
        if not isinstance(row, dict):
            continue
        sym = str(row.get("base_symbol", "")).strip().lower()
        aid = normalize_hex_id(str(row.get("asset_id", "")))
        if sym and aid:
            out[sym] = aid
    return out


def _resolve_market_base_cat_asset_id(*, base_asset: str, symbol_map: dict[str, str]) -> str | None:
    normalized = normalize_hex_id(base_asset)
    if normalized:
        return normalized
    key = str(base_asset or "").strip().lower()
    return symbol_map.get(key)


def _build_enabled_cat_jobs(
    *,
    markets_config_path: Path,
    testnet_markets_path: Path | None,
    cats_path: Path,
    only_cat_asset_id: str | None,
) -> list[CatDustJob]:
    cfg = load_markets_config_with_optional_overlay(
        path=markets_config_path.expanduser(),
        overlay_path=testnet_markets_path.expanduser() if testnet_markets_path else None,
    )
    symbol_map = _load_symbol_to_cat_asset_id(cats_path.expanduser())
    filter_id = normalize_hex_id(only_cat_asset_id) if only_cat_asset_id else None

    grouped: dict[tuple[str, str], dict[str, Any]] = {}
    for m in cfg.markets:
        if not m.enabled:
            continue
        aid = _resolve_market_base_cat_asset_id(base_asset=m.base_asset, symbol_map=symbol_map)
        if not aid:
            continue
        if filter_id and aid != filter_id:
            continue
        sk = str(m.signer_key_id).strip()
        key = (sk, aid)
        if key not in grouped:
            grouped[key] = {"receive": m.receive_address, "markets": [m.market_id]}
            continue
        if grouped[key]["receive"] != m.receive_address:
            raise ValueError(
                f"Conflicting receive_address for signer={sk!r} cat={aid}: "
                f"{grouped[key]['receive']!r} vs {m.receive_address!r} "
                f"(markets {grouped[key]['markets']!r} vs {m.market_id!r})"
            )
        grouped[key]["markets"].append(m.market_id)

    jobs: list[CatDustJob] = []
    for (signer_key_id, cat_asset_id), payload in sorted(grouped.items()):
        markets = tuple(sorted(set(payload["markets"])))
        jobs.append(
            CatDustJob(
                cat_asset_id=cat_asset_id,
                signer_key_id=signer_key_id,
                receive_address=str(payload["receive"]),
                market_ids=markets,
            )
        )
    return jobs


def _dust_coin_ids_from_list_payload(
    payload: dict[str, Any], *, dust_threshold_mojos: int
) -> list[str]:
    coins = payload.get("coins")
    if not isinstance(coins, list):
        return []
    out: list[str] = []
    for row in coins:
        if not isinstance(row, dict):
            continue
        if str(row.get("type", "")).upper() != "CAT":
            continue
        if int(row.get("spent_block_index", 0) or 0) != 0:
            continue
        amount = int(row.get("amount", 0) or 0)
        if amount <= 0 or amount >= dust_threshold_mojos:
            continue
        cid = normalize_hex_id(str(row.get("coin_id", "")))
        if cid:
            out.append(cid)
    return out


def _chunk_coin_ids(coin_ids: list[str], batch_size: int) -> list[list[str]]:
    size = max(2, int(batch_size))
    return [coin_ids[i : i + size] for i in range(0, len(coin_ids), size)]


def _run_script_json(
    *,
    argv: list[str],
    cwd: Path,
) -> tuple[int, dict[str, Any] | None, str]:
    proc = subprocess.run(
        [sys.executable, *argv],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        check=False,
    )
    err_tail = (proc.stderr or "").strip()
    if proc.returncode != 0:
        return proc.returncode, None, err_tail
    try:
        return 0, json.loads(proc.stdout), err_tail
    except json.JSONDecodeError:
        return 1, None, (proc.stdout[:2000] + err_tail).strip()


def _list_argv_common(args: argparse.Namespace, program: Any) -> list[str]:
    root = _repo_root()
    list_script = root / "scripts" / "list_vault_coins_coinset.py"
    argv: list[str] = [str(list_script)]
    argv.extend(["--network", str(args.network).strip()])
    if str(args.coinset_base_url).strip():
        argv.extend(["--coinset-base-url", str(args.coinset_base_url).strip()])
    if str(args.launcher_id).strip():
        argv.extend(["--launcher-id", normalize_hex_id(str(args.launcher_id))])
    if str(args.launcher_id_file).strip():
        argv.extend(["--launcher-id-file", str(Path(args.launcher_id_file).expanduser())])
    argv.extend(
        [
            "--cloud-wallet-base-url",
            str(program.cloud_wallet_base_url).strip(),
            "--cloud-wallet-user-key-id",
            str(program.cloud_wallet_user_key_id).strip(),
            "--cloud-wallet-private-key-pem-path",
            str(Path(program.cloud_wallet_private_key_pem_path).expanduser()),
            "--vault-id",
            str(program.cloud_wallet_vault_id).strip(),
            "--max-nonce",
            str(int(args.max_nonce)),
        ]
    )
    return argv


def _combine_argv_for_batch(
    *,
    args: argparse.Namespace,
    program: Any,
    key_id: str,
    keyring_yaml_path: str,
    receive_address: str,
    coin_names: list[str],
) -> list[str]:
    root = _repo_root()
    combine_script = root / "scripts" / "combine_coinset_direct.py"
    argv: list[str] = [str(combine_script)]
    for cid in coin_names:
        argv.extend(["--coin-name", cid])
    argv.extend(
        [
            "--max-input-coins",
            str(len(coin_names)),
            "--network",
            str(args.network).strip(),
            "--key-id",
            key_id,
            "--keyring-yaml-path",
            str(Path(keyring_yaml_path).expanduser()),
            "--receive-address",
            receive_address,
            "--cloud-wallet-base-url",
            str(program.cloud_wallet_base_url).strip(),
            "--cloud-wallet-user-key-id",
            str(program.cloud_wallet_user_key_id).strip(),
            "--cloud-wallet-private-key-pem-path",
            str(Path(program.cloud_wallet_private_key_pem_path).expanduser()),
            "--vault-id",
            str(program.cloud_wallet_vault_id).strip(),
            "--cloud-wallet-kms-key-id",
            str(program.cloud_wallet_kms_key_id).strip(),
            "--cloud-wallet-kms-region",
            str(program.cloud_wallet_kms_region or "us-west-2").strip(),
        ]
    )
    kms_pk = str(program.cloud_wallet_kms_public_key_hex or "").strip()
    if kms_pk:
        argv.extend(["--cloud-wallet-kms-public-key-hex", kms_pk])
    argv.extend(
        [
            "--verify-timeout-seconds",
            str(int(args.verify_timeout_seconds)),
            "--verify-poll-seconds",
            str(int(args.verify_poll_seconds)),
        ]
    )
    if str(args.coinset_base_url).strip():
        argv.extend(["--coinset-base-url", str(args.coinset_base_url).strip()])
    return argv


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description=(
            "List vault CAT coins (Coinset) for enabled-market assets and combine "
            "sub-unit dust via combine_coinset_direct."
        )
    )
    p.add_argument(
        "--program-config",
        default="~/.greenfloor/config/program.yaml",
        help="Path to program.yaml (Cloud Wallet, KMS, key registry).",
    )
    p.add_argument(
        "--markets-config",
        default="~/.greenfloor/config/markets.yaml",
        help="Base markets.yaml path.",
    )
    p.add_argument(
        "--testnet-markets-config",
        default="",
        help="Optional testnet overlay (merged after base), same semantics as manager/daemon.",
    )
    p.add_argument(
        "--cats-config",
        default="",
        help="cats.yaml path (default: ~/.greenfloor/config/cats.yaml or config/cats.yaml).",
    )
    p.add_argument(
        "--network",
        default="",
        help="Override network (default: app.network from program.yaml).",
    )
    p.add_argument("--coinset-base-url", default="", help="Optional Coinset base URL override.")
    p.add_argument("--launcher-id", default="", help="Vault singleton launcher id hex (optional).")
    p.add_argument(
        "--launcher-id-file",
        default="~/.greenfloor/cache/vault_launcher_id.txt",
        help="Launcher id cache file (read/write).",
    )
    p.add_argument(
        "--dust-threshold-mojos",
        type=int,
        default=1000,
        help="Coins below this mojo amount are dust (default 1000 = one CAT unit).",
    )
    p.add_argument(
        "--max-input-coins",
        type=int,
        default=10,
        help="Maximum inputs per combine_coinset_direct batch (default 10).",
    )
    p.add_argument(
        "--max-nonce",
        type=int,
        default=120,
        help="list_vault_coins_coinset --max-nonce (default 120).",
    )
    p.add_argument(
        "--cat-asset-id",
        default="",
        help="If set, only process this CAT asset id (must still appear on an enabled market).",
    )
    p.add_argument(
        "--dry-run",
        action="store_true",
        help="Run listing and print combine batches; do not invoke combine_coinset_direct.",
    )
    p.add_argument(
        "--list-only",
        action="store_true",
        help="Only run per-asset listing + dust selection (no combines).",
    )
    p.add_argument("--verify-timeout-seconds", type=int, default=15 * 60)
    p.add_argument("--verify-poll-seconds", type=int, default=8)
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    program_path = Path(args.program_config).expanduser()
    program = load_program_config(program_path)
    network = str(args.network).strip() or program.app_network
    if network.lower() in {"testnet"}:
        network = "testnet11"
    args.network = network

    cats_arg = str(args.cats_config).strip()
    cats_default = default_cats_config_path()
    cats_path: Path | None = Path(cats_arg).expanduser() if cats_arg else cats_default
    if cats_path is None or not cats_path.is_file():
        print(
            json.dumps(
                {
                    "status": "error",
                    "reason": "cats_config_missing",
                    "detail": cats_arg or "default cats.yaml resolution failed",
                },
                indent=2,
            ),
            file=sys.stderr,
        )
        return 1

    overlay: Path | None = None
    if str(args.testnet_markets_config).strip():
        overlay = Path(args.testnet_markets_config).expanduser()

    try:
        jobs = _build_enabled_cat_jobs(
            markets_config_path=Path(args.markets_config),
            testnet_markets_path=overlay,
            cats_path=cats_path,
            only_cat_asset_id=str(args.cat_asset_id).strip() or None,
        )
    except ValueError as exc:
        print(json.dumps({"status": "error", "reason": "config", "detail": str(exc)}, indent=2))
        return 1

    if not jobs:
        print(
            json.dumps(
                {
                    "status": "ok",
                    "message": "no_enabled_cat_markets",
                    "network": args.network,
                    "jobs": [],
                },
                indent=2,
            )
        )
        return 0

    root = _repo_root()
    report: dict[str, Any] = {
        "network": args.network,
        "dust_threshold_mojos": int(args.dust_threshold_mojos),
        "dry_run": bool(args.dry_run),
        "list_only": bool(args.list_only),
        "jobs": [],
    }
    exit_code = 0
    combine_mode = not bool(args.dry_run) and not bool(args.list_only)

    for job in jobs:
        signer = program.signer_key_registry.get(job.signer_key_id)
        keyring = str(signer.keyring_yaml_path or "").strip() if signer else ""
        if combine_mode:
            if signer is None:
                exit_code = 1
                report["jobs"].append(
                    {
                        "cat_asset_id": job.cat_asset_id,
                        "signer_key_id": job.signer_key_id,
                        "status": "error",
                        "reason": "unknown_signer_key_id",
                        "market_ids": list(job.market_ids),
                    }
                )
                continue
            if not keyring:
                exit_code = 1
                report["jobs"].append(
                    {
                        "cat_asset_id": job.cat_asset_id,
                        "signer_key_id": job.signer_key_id,
                        "status": "error",
                        "reason": "missing_keyring_yaml_path",
                        "market_ids": list(job.market_ids),
                    }
                )
                continue

        list_argv = _list_argv_common(args, program)
        list_argv.extend(["--asset-type", "cat", "--cat-id", job.cat_asset_id])

        code, list_payload, list_err = _run_script_json(argv=list_argv, cwd=root)
        job_report: dict[str, Any] = {
            "cat_asset_id": job.cat_asset_id,
            "signer_key_id": job.signer_key_id,
            "market_ids": list(job.market_ids),
            "receive_address": job.receive_address,
            "list": {"exit_code": code, "stderr_tail": list_err or None},
        }
        if code != 0 or not isinstance(list_payload, dict):
            job_report["status"] = "error"
            job_report["reason"] = "list_failed"
            exit_code = 1
            report["jobs"].append(job_report)
            continue

        job_report["list"]["summary"] = {
            "count": list_payload.get("count"),
            "max_nonce_scanned": list_payload.get("max_nonce_scanned"),
            "launcher_id": list_payload.get("launcher_id"),
        }
        dust_ids = _dust_coin_ids_from_list_payload(
            list_payload, dust_threshold_mojos=int(args.dust_threshold_mojos)
        )
        job_report["dust_coin_count"] = len(dust_ids)
        batches = _chunk_coin_ids(dust_ids, int(args.max_input_coins))
        job_report["combine_batches_planned"] = len(batches)

        if bool(args.list_only) or bool(args.dry_run):
            job_report["status"] = "ok"
            can_combine = bool(signer and keyring)
            job_report["signer_config_ok"] = can_combine
            if not can_combine:
                job_report["signer_config_note"] = (
                    "unknown_signer_key_id" if signer is None else "missing_keyring_yaml_path"
                )
            job_report["batches"] = [
                {
                    "coin_ids": b,
                    "would_combine": len(b) >= 2 and can_combine,
                }
                for b in batches
            ]
            report["jobs"].append(job_report)
            continue

        batch_results: list[dict[str, Any]] = []
        job_failed = False
        for batch in batches:
            if len(batch) < 2:
                batch_results.append(
                    {"coin_ids": batch, "skipped": True, "reason": "need_at_least_2"}
                )
                continue
            c_argv = _combine_argv_for_batch(
                args=args,
                program=program,
                key_id=job.signer_key_id,
                keyring_yaml_path=keyring,
                receive_address=job.receive_address,
                coin_names=batch,
            )
            c_code, c_payload, c_err = _run_script_json(argv=c_argv, cwd=root)
            entry = {
                "coin_ids": batch,
                "exit_code": c_code,
                "stderr_tail": c_err or None,
                "payload": c_payload,
            }
            batch_results.append(entry)
            if c_code != 0:
                exit_code = 1
                job_failed = True
        job_report["status"] = "error" if job_failed else "ok"
        job_report["batches"] = batch_results
        report["jobs"].append(job_report)

    report["status"] = "error" if exit_code != 0 else "ok"
    print(json.dumps(report, indent=2))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
