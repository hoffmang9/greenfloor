"""Vault Coinset scanning helpers and CLI entry point."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from greenfloor_scripts.config_subprocess import (
    all_market_rows,
    ensure_program_config_valid,
    launcher_id_from_program_config,
    load_cats_fields,
    load_markets_fields,
)
from greenfloor_scripts.hex_subprocess import is_hex_id, normalize_hex_id

from scripts.vault_coinset_scan_checkpoint import (
    CoinRow,
    _clear_cache_files,
    _load_scan_checkpoint,
    _save_scan_checkpoint,
)
from scripts.vault_coinset_scan_coinset import (
    CoinsetScanner,
    _chunk_values,
    _coin_id_from_record,
    _coinset_with_retries,
    _detect_cat_asset_id,
    _hex_to_bytes,
    _import_sdk,
    _safe_int,
    _to_coinset_hex,
)


def _read_launcher_id_file(path: str) -> str:
    if not str(path).strip():
        return ""
    file_path = Path(path).expanduser()
    if not file_path.exists():
        return ""
    raw = file_path.read_text(encoding="utf-8").strip()
    return normalize_hex_id(raw) or ""


def _write_launcher_id_file(path: str, launcher_id: str) -> None:
    file_path = Path(path).expanduser()
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(f"{launcher_id}\n", encoding="utf-8")


def _normalize_label(value: object) -> str:
    return "".join(ch for ch in str(value).strip().lower() if ch.isalnum())


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _load_cat_metadata_indexes() -> tuple[dict[str, set[str]], dict[str, list[str]]]:
    ticker_to_asset_ids: dict[str, set[str]] = {}
    asset_id_to_symbols: dict[str, set[str]] = {}

    def add_mapping(*, ticker: object, asset_id: object) -> None:
        clean_asset_id = normalize_hex_id(asset_id)
        clean_ticker = _normalize_label(ticker)
        if not clean_asset_id or not clean_ticker:
            return
        ticker_to_asset_ids.setdefault(clean_ticker, set()).add(clean_asset_id)
        asset_id_to_symbols.setdefault(clean_asset_id, set()).add(str(ticker).strip())

    cats_path = _repo_root() / "config" / "cats.yaml"
    if cats_path.exists():
        try:
            cats_fields = load_cats_fields(cats_config=cats_path)
        except Exception:
            cats_fields = {}
        cats = cats_fields.get("cats") if isinstance(cats_fields, dict) else None
        if isinstance(cats, list):
            for item in cats:
                if not isinstance(item, dict):
                    continue
                asset_id = item.get("asset_id")
                add_mapping(ticker=item.get("base_symbol"), asset_id=asset_id)
                add_mapping(ticker=item.get("name"), asset_id=asset_id)
                aliases = item.get("aliases")
                if isinstance(aliases, list):
                    for alias in aliases:
                        add_mapping(ticker=alias, asset_id=asset_id)

    markets_path = _repo_root() / "config" / "markets.yaml"
    if markets_path.exists():
        try:
            markets_fields = load_markets_fields(markets_config=markets_path)
        except Exception:
            markets_fields = {}
        markets = all_market_rows(markets_fields)
        for market in markets:
            add_mapping(ticker=market.get("base_symbol"), asset_id=market.get("base_asset"))
            quote_asset = str(market.get("quote_asset", "")).strip()
            if is_hex_id(quote_asset):
                add_mapping(ticker=quote_asset, asset_id=quote_asset)

    frozen_asset_to_symbols = {k: sorted(v) for k, v in asset_id_to_symbols.items()}
    return ticker_to_asset_ids, frozen_asset_to_symbols


def _parse_csv_values(values: list[str]) -> list[str]:
    parsed: list[str] = []
    for value in values:
        parts = [segment.strip() for segment in str(value).split(",")]
        parsed.extend([part for part in parts if part])
    return parsed


def _resolve_requested_cat_ids(
    *,
    cat_ids: list[str],
    cat_tickers: list[str],
    ticker_to_asset_ids: dict[str, set[str]],
) -> tuple[set[str], list[str]]:
    resolved: set[str] = set()
    unresolved_tickers: list[str] = []
    for raw_id in cat_ids:
        clean = normalize_hex_id(raw_id)
        if clean:
            resolved.add(clean)
    for ticker in cat_tickers:
        key = _normalize_label(ticker)
        matches = ticker_to_asset_ids.get(key, set())
        if not matches:
            unresolved_tickers.append(str(ticker).strip())
            continue
        resolved.update(matches)
    return resolved, unresolved_tickers


def main() -> int:
    parser = argparse.ArgumentParser(
        description="List vault coins via Coinset using only vault singleton launcher id.",
    )
    parser.add_argument("--network", default="mainnet", choices=["mainnet", "testnet11", "testnet"])
    parser.add_argument("--coinset-base-url", default="")
    parser.add_argument(
        "--launcher-id",
        default="",
        help="Optional vault launcher id hex; read from program config when omitted.",
    )
    parser.add_argument(
        "--launcher-id-file",
        default="",
        help="Read launcher id from this file when --launcher-id is omitted; resolved launcher id is saved here too.",
    )
    parser.add_argument(
        "--program-config",
        default="",
        help="Path to program.yaml used to resolve vault.launcher_id when --launcher-id is omitted.",
    )
    parser.add_argument(
        "--resolve-launcher-id-only",
        action="store_true",
        help="Resolve launcher id (from arg/file/program config), print it, then exit.",
    )
    parser.add_argument("--max-nonce", type=int, default=100)
    parser.add_argument("--include-spent", action="store_true")
    parser.add_argument("--asset-type", default="all", choices=["all", "xch", "cat"])
    parser.add_argument(
        "--cat-id",
        action="append",
        default=[],
        help="CAT asset id filter (repeat or comma-separate). Implies --asset-type cat.",
    )
    parser.add_argument(
        "--cat-ticker",
        action="append",
        default=[],
        help="CAT ticker/symbol filter from config metadata (repeat or comma-separate). Implies --asset-type cat.",
    )
    parser.add_argument("--cat-asset-id", default="")
    parser.add_argument(
        "--checkpoint-file",
        default="",
        help=(
            "Optional JSON checkpoint/cache file. "
            "When set, nonce scan progress and CAT classification cache are resumed across runs."
        ),
    )
    parser.add_argument(
        "--checkpoint-save-interval",
        type=int,
        default=1,
        help="Save checkpoint after every N nonce scans (default 1).",
    )
    parser.add_argument(
        "--no-resume-checkpoint",
        action="store_true",
        help="Ignore existing checkpoint contents and start scanning from nonce 0.",
    )
    parser.add_argument(
        "--nonce-batch-size",
        type=int,
        default=32,
        help="Nonce scan batch size for Coinset puzzle_hashes/hints queries (default 32).",
    )
    parser.add_argument(
        "--empty-batch-stop-count",
        type=int,
        default=1,
        help="Stop after this many consecutive empty nonce batches beyond nonce 0 (default 1).",
    )
    parser.add_argument(
        "--parent-lookup-batch-size",
        type=int,
        default=64,
        help="Parent coin lookup batch size for get_coin_records_by_names (default 64).",
    )
    parser.add_argument(
        "--start-height",
        type=int,
        default=None,
        help="Optional Coinset start_height filter for coin record queries.",
    )
    parser.add_argument(
        "--end-height",
        type=int,
        default=None,
        help="Optional Coinset end_height filter for coin record queries.",
    )
    parser.add_argument(
        "--incremental-from-checkpoint",
        action="store_true",
        help=(
            "When checkpointing is enabled, continue from checkpoint last_synced_height + 1 "
            "and cap end_height to current chain peak when omitted."
        ),
    )
    parser.add_argument(
        "--auto-increment",
        action="store_true",
        help=(
            "Convenience mode: enables checkpointing and incremental-from-checkpoint. "
            "Uses ~/.greenfloor/cache/vault_coinset_checkpoint.json when --checkpoint-file is unset."
        ),
    )
    parser.add_argument(
        "--clear-caches",
        action="store_true",
        help=(
            "Clear launcher/checkpoint cache files before scanning. "
            "Targets launcher-id-file/checkpoint-file when provided, otherwise default cache paths."
        ),
    )
    args = parser.parse_args()

    program_config = str(args.program_config).strip()
    if program_config:
        ensure_program_config_valid(program_config=Path(program_config).expanduser())
    else:
        ensure_program_config_valid()

    if bool(args.auto_increment):
        if bool(args.no_resume_checkpoint):
            raise ValueError("cannot use --auto-increment with --no-resume-checkpoint")
        if not str(args.checkpoint_file).strip():
            args.checkpoint_file = "~/.greenfloor/cache/vault_coinset_checkpoint.json"
        args.incremental_from_checkpoint = True
    if bool(args.clear_caches):
        cache_files = [
            str(args.launcher_id_file).strip() or "~/.greenfloor/cache/vault_launcher_id.txt",
            str(args.checkpoint_file).strip()
            or "~/.greenfloor/cache/vault_coinset_checkpoint.json",
        ]
        cache_clear_result = _clear_cache_files(cache_files)
    else:
        cache_clear_result = {}

    launcher_id = normalize_hex_id(args.launcher_id)
    launcher_id_source = "arg"
    if not launcher_id and str(args.launcher_id_file).strip():
        launcher_id = _read_launcher_id_file(args.launcher_id_file)
        if launcher_id:
            launcher_id_source = "file"
    if not launcher_id:
        program_config = str(args.program_config).strip()
        if not program_config:
            raise ValueError("launcher-id, launcher-id-file, or --program-config is required")
        launcher_id = launcher_id_from_program_config(program_config)
        launcher_id_source = "program_config"
    if str(args.launcher_id_file).strip() and launcher_id_source in {"program_config", "arg"}:
        _write_launcher_id_file(args.launcher_id_file, launcher_id)

    if bool(args.resolve_launcher_id_only):
        print(
            json.dumps(
                {
                    "launcher_id": launcher_id,
                    "launcher_id_source": launcher_id_source,
                    "launcher_id_file": str(Path(args.launcher_id_file).expanduser())
                    if str(args.launcher_id_file).strip()
                    else None,
                },
                indent=2,
            )
        )
        return 0

    sdk = _import_sdk()
    scanner = CoinsetScanner(network=args.network, base_url=args.coinset_base_url or None)
    ticker_to_asset_ids, asset_id_to_symbols = _load_cat_metadata_indexes()
    requested_cat_ids_raw = _parse_csv_values(args.cat_id)
    requested_cat_tickers_raw = _parse_csv_values(args.cat_ticker)
    if str(args.cat_asset_id).strip():
        requested_cat_ids_raw.append(str(args.cat_asset_id).strip())
    requested_cat_ids, unresolved_cat_tickers = _resolve_requested_cat_ids(
        cat_ids=requested_cat_ids_raw,
        cat_tickers=requested_cat_tickers_raw,
        ticker_to_asset_ids=ticker_to_asset_ids,
    )
    if unresolved_cat_tickers:
        raise ValueError(f"unknown cat ticker(s): {', '.join(unresolved_cat_tickers)}")
    effective_asset_type = (
        "cat"
        if requested_cat_ids or requested_cat_tickers_raw
        else str(args.asset_type).strip().lower()
    )

    max_nonce_target = max(0, int(args.max_nonce))
    checkpoint_file = str(args.checkpoint_file).strip()
    checkpoint_save_interval = max(1, int(args.checkpoint_save_interval))
    checkpoint_enabled = bool(checkpoint_file)
    checkpoint_resumed = False
    checkpoint_start_nonce = 0

    by_coin_id: dict[str, CoinRow]
    nonce_to_p2: dict[int, str]
    cat_asset_cache: dict[str, str]
    parent_lineage_cache: dict[str, dict[str, Any]]
    checkpoint_last_synced_height: int | None
    if checkpoint_enabled and not bool(args.no_resume_checkpoint):
        (
            checkpoint_start_nonce,
            nonce_to_p2,
            by_coin_id,
            cat_asset_cache,
            parent_lineage_cache,
            checkpoint_last_synced_height,
        ) = _load_scan_checkpoint(
            checkpoint_file=checkpoint_file,
            network=args.network,
            launcher_id=launcher_id,
            include_spent=bool(args.include_spent),
        )
        checkpoint_resumed = (
            checkpoint_start_nonce > 0
            or bool(by_coin_id)
            or bool(cat_asset_cache)
            or bool(parent_lineage_cache)
        )
    else:
        by_coin_id = {}
        nonce_to_p2 = {}
        cat_asset_cache = {}
        parent_lineage_cache = {}
        checkpoint_last_synced_height = None

    nonce_batch_size = max(1, int(args.nonce_batch_size))
    empty_batch_stop_count = max(1, int(args.empty_batch_stop_count))
    parent_lookup_batch_size = max(1, int(args.parent_lookup_batch_size))
    requested_start_height = (
        args.start_height if args.start_height is None else max(0, int(args.start_height))
    )
    requested_end_height = (
        args.end_height if args.end_height is None else max(0, int(args.end_height))
    )
    if requested_start_height is not None and requested_end_height is not None:
        if requested_end_height < requested_start_height:
            raise ValueError("end-height must be greater than or equal to start-height")
    if bool(args.incremental_from_checkpoint) and requested_start_height is not None:
        raise ValueError("cannot use --start-height with --incremental-from-checkpoint")
    if bool(args.incremental_from_checkpoint) and not checkpoint_enabled:
        raise ValueError("--incremental-from-checkpoint requires --checkpoint-file")

    chain_peak_height: int | None = None
    if bool(args.incremental_from_checkpoint) or requested_end_height is None:
        state = _coinset_with_retries(lambda: scanner.adapter.get_blockchain_state())
        if isinstance(state, dict):
            peak_raw = state.get("peak")
            if isinstance(peak_raw, dict):
                chain_peak_height = _safe_int(peak_raw.get("height"), default=-1)
            if chain_peak_height is None or chain_peak_height < 0:
                chain_peak_height = _safe_int(state.get("peak_height"), default=-1)
            if chain_peak_height is not None and chain_peak_height < 0:
                chain_peak_height = None

    effective_start_height = requested_start_height
    if bool(args.incremental_from_checkpoint):
        if checkpoint_last_synced_height is not None and checkpoint_last_synced_height >= 0:
            effective_start_height = int(checkpoint_last_synced_height) + 1
        elif effective_start_height is None:
            effective_start_height = 0
    effective_end_height = requested_end_height
    if effective_end_height is None and chain_peak_height is not None:
        effective_end_height = int(chain_peak_height)
    checkpoint_synced_height = (
        int(effective_end_height)
        if effective_end_height is not None
        else (int(chain_peak_height) if chain_peak_height is not None else None)
    )

    if (
        effective_start_height is not None
        and effective_end_height is not None
        and effective_start_height > effective_end_height
    ):
        stop_reason = "scan_window_exhausted"
        print(
            json.dumps(
                {
                    "network": scanner.adapter.network,
                    "coinset_base_url": scanner.adapter.base_url,
                    "launcher_id": launcher_id,
                    "asset_type": effective_asset_type,
                    "requested_cat_ids": sorted(requested_cat_ids),
                    "requested_cat_tickers": sorted(set(requested_cat_tickers_raw)),
                    "max_nonce_scanned": max(nonce_to_p2.keys()) if nonce_to_p2 else 0,
                    "count": 0,
                    "checkpoint": {
                        "enabled": checkpoint_enabled,
                        "file": str(Path(checkpoint_file).expanduser())
                        if checkpoint_enabled
                        else None,
                        "resumed": checkpoint_resumed,
                        "start_nonce": checkpoint_start_nonce,
                        "save_interval": checkpoint_save_interval if checkpoint_enabled else None,
                        "cat_asset_cache_entries": len(cat_asset_cache),
                        "parent_lineage_cache_entries": len(parent_lineage_cache),
                        "last_synced_height": checkpoint_last_synced_height,
                    },
                    "scan_batches": {
                        "nonce_batch_size": nonce_batch_size,
                        "empty_batch_stop_count": empty_batch_stop_count,
                        "parent_lookup_batch_size": parent_lookup_batch_size,
                    },
                    "scan_window": {
                        "start_height": effective_start_height,
                        "end_height": effective_end_height,
                        "chain_peak_height": chain_peak_height,
                        "incremental_from_checkpoint": bool(args.incremental_from_checkpoint),
                        "auto_increment": bool(args.auto_increment),
                    },
                    "scan_stop_reason": stop_reason,
                    "coins": [],
                },
                indent=2,
            )
        )
        return 0

    scanned_since_resume = 0
    empty_batch_count = 0
    stop_reason = "max_nonce_reached"
    for batch_start in range(checkpoint_start_nonce, max_nonce_target + 1, nonce_batch_size):
        batch_end = min(batch_start + nonce_batch_size - 1, max_nonce_target)
        batch_nonces = list(range(batch_start, batch_end + 1))
        nonce_p2: dict[int, str] = {}
        for nonce in batch_nonces:
            cfg = sdk.MemberConfig().with_top_level(True).with_nonce(int(nonce))
            p2_hash = normalize_hex_id(
                sdk.to_hex(sdk.singleton_member_hash(cfg, _hex_to_bytes(launcher_id), False))
            )
            if p2_hash:
                nonce_p2[nonce] = p2_hash
                nonce_to_p2[nonce] = p2_hash
        p2_hashes = list(
            dict.fromkeys(_to_coinset_hex(_hex_to_bytes(v)) for v in nonce_p2.values())
        )
        by_puzzle = scanner.by_puzzle_hashes(
            puzzle_hashes=p2_hashes,
            include_spent=args.include_spent,
            start_height=effective_start_height,
            end_height=effective_end_height,
        )
        by_hint = scanner.by_hints(
            hints=p2_hashes,
            include_spent=args.include_spent,
            start_height=effective_start_height,
            end_height=effective_end_height,
        )
        batch_has_any = bool(by_puzzle) or bool(by_hint)
        if batch_end > 0 and not batch_has_any:
            empty_batch_count += 1
        else:
            empty_batch_count = 0
        if empty_batch_count >= empty_batch_stop_count:
            stop_reason = "empty_nonce_batches"
            if checkpoint_enabled:
                _save_scan_checkpoint(
                    checkpoint_file=checkpoint_file,
                    network=args.network,
                    launcher_id=launcher_id,
                    include_spent=bool(args.include_spent),
                    max_nonce_completed=batch_end,
                    nonce_to_p2=nonce_to_p2,
                    by_coin_id=by_coin_id,
                    cat_asset_cache=cat_asset_cache,
                    parent_lineage_cache=parent_lineage_cache,
                    last_synced_height=checkpoint_synced_height,
                    scan_start_height=effective_start_height,
                    scan_end_height=effective_end_height,
                )
            break

        for source, records in (("puzzle_hash", by_puzzle), ("hint", by_hint)):
            for record in records:
                coin_id = _coin_id_from_record(record)
                if not coin_id:
                    continue
                coin_raw = record.get("coin")
                coin: dict[str, Any] = coin_raw if isinstance(coin_raw, dict) else {}
                row = by_coin_id.get(coin_id)
                if row is None:
                    row = CoinRow(
                        coin_id=coin_id,
                        puzzle_hash=normalize_hex_id(coin.get("puzzle_hash")) or "",
                        parent_coin_info=normalize_hex_id(coin.get("parent_coin_info")) or "",
                        amount=_safe_int(coin.get("amount"), default=0),
                        confirmed_block_index=_safe_int(
                            record.get("confirmed_block_index"), default=0
                        ),
                        spent_block_index=_safe_int(record.get("spent_block_index"), default=0),
                        discovered_nonces=[],
                        discovered_by_puzzle_hash=False,
                        discovered_by_hint=False,
                        coin_type="UNKNOWN",
                        cat_asset_id=None,
                        cat_symbols=[],
                    )
                    by_coin_id[coin_id] = row
                for nonce, batch_p2 in nonce_p2.items():
                    if row.puzzle_hash == batch_p2 and nonce not in row.discovered_nonces:
                        row.discovered_nonces.append(nonce)
                row.discovered_nonces.sort()
                if source == "puzzle_hash":
                    row.discovered_by_puzzle_hash = True
                if source == "hint":
                    row.discovered_by_hint = True

        scanned_since_resume += len(batch_nonces)
        if checkpoint_enabled and (
            scanned_since_resume % checkpoint_save_interval == 0 or batch_end >= max_nonce_target
        ):
            _save_scan_checkpoint(
                checkpoint_file=checkpoint_file,
                network=args.network,
                launcher_id=launcher_id,
                include_spent=bool(args.include_spent),
                max_nonce_completed=batch_end,
                nonce_to_p2=nonce_to_p2,
                by_coin_id=by_coin_id,
                cat_asset_cache=cat_asset_cache,
                parent_lineage_cache=parent_lineage_cache,
                last_synced_height=checkpoint_synced_height,
                scan_start_height=effective_start_height,
                scan_end_height=effective_end_height,
            )

    parent_record_cache: dict[str, dict[str, Any] | None] = {}
    puzzle_solution_cache: dict[str, dict[str, Any] | None] = {}
    unresolved_parent_ids = sorted(
        {
            row.parent_coin_info
            for row in by_coin_id.values()
            if row.parent_coin_info and row.parent_coin_info not in parent_record_cache
        }
    )
    for parent_batch in _chunk_values(unresolved_parent_ids, parent_lookup_batch_size):
        parent_records = scanner.by_names(
            coin_names=[_to_coinset_hex(_hex_to_bytes(parent_id)) for parent_id in parent_batch],
            include_spent=True,
        )
        for parent in parent_records:
            parent_id = _coin_id_from_record(parent)
            if parent_id:
                parent_record_cache[parent_id] = parent

    for row in by_coin_id.values():
        p2_hashes = {nonce_to_p2.get(nonce, "") for nonce in row.discovered_nonces}
        if row.puzzle_hash and row.puzzle_hash in p2_hashes:
            row.coin_type = "XCH"
            continue
        cached_asset_id = cat_asset_cache.get(row.coin_id)
        if cached_asset_id is not None:
            if cached_asset_id:
                row.coin_type = "CAT"
                row.cat_asset_id = cached_asset_id
                row.cat_symbols = list(asset_id_to_symbols.get(cached_asset_id, []))
            else:
                row.coin_type = "OTHER"
            continue
        record = {
            "coin": {
                "parent_coin_info": row.parent_coin_info,
                "puzzle_hash": row.puzzle_hash,
                "amount": row.amount,
            },
        }
        cat_asset_id = _detect_cat_asset_id(
            sdk=sdk,
            coinset=scanner,
            coin_id=row.coin_id,
            record=record,
            cat_asset_cache=cat_asset_cache,
            parent_record_cache=parent_record_cache,
            puzzle_solution_cache=puzzle_solution_cache,
            parent_lineage_cache=parent_lineage_cache,
        )
        if cat_asset_id:
            row.coin_type = "CAT"
            row.cat_asset_id = cat_asset_id
            row.cat_symbols = list(asset_id_to_symbols.get(cat_asset_id, []))
            continue
        row.coin_type = "OTHER"

    max_nonce_scanned = max(nonce_to_p2.keys()) if nonce_to_p2 else 0
    pre_verify_count = len(by_coin_id)
    verified_coin_ids = scanner.existing_coin_names(coin_ids_hex=sorted(by_coin_id.keys()))
    verification_applied = bool(verified_coin_ids)
    if verification_applied:
        by_coin_id = {
            coin_id: row for coin_id, row in by_coin_id.items() if coin_id in verified_coin_ids
        }
    dropped_unverified_count = (
        max(0, pre_verify_count - len(by_coin_id)) if verification_applied else 0
    )
    if checkpoint_enabled:
        _save_scan_checkpoint(
            checkpoint_file=checkpoint_file,
            network=args.network,
            launcher_id=launcher_id,
            include_spent=bool(args.include_spent),
            max_nonce_completed=max_nonce_scanned,
            nonce_to_p2=nonce_to_p2,
            by_coin_id=by_coin_id,
            cat_asset_cache=cat_asset_cache,
            parent_lineage_cache=parent_lineage_cache,
            last_synced_height=checkpoint_synced_height,
            scan_start_height=effective_start_height,
            scan_end_height=effective_end_height,
        )

    filtered: list[CoinRow] = []
    for row in sorted(by_coin_id.values(), key=lambda r: (r.coin_type, r.amount, r.coin_id)):
        if effective_asset_type == "xch" and row.coin_type != "XCH":
            continue
        if effective_asset_type == "cat" and row.coin_type != "CAT":
            continue
        if requested_cat_ids and (row.cat_asset_id or "") not in requested_cat_ids:
            continue
        filtered.append(row)

    print(
        json.dumps(
            {
                "network": scanner.adapter.network,
                "coinset_base_url": scanner.adapter.base_url,
                "launcher_id": launcher_id,
                "asset_type": effective_asset_type,
                "requested_cat_ids": sorted(requested_cat_ids),
                "requested_cat_tickers": sorted(set(requested_cat_tickers_raw)),
                "max_nonce_scanned": max_nonce_scanned,
                "count": len(filtered),
                "name_verification": {
                    "applied": verification_applied,
                    "pre_verify_count": pre_verify_count,
                    "verified_count": len(by_coin_id) if verification_applied else None,
                    "dropped_unverified_count": dropped_unverified_count,
                },
                "cache_clear": cache_clear_result or None,
                "checkpoint": {
                    "enabled": checkpoint_enabled,
                    "file": str(Path(checkpoint_file).expanduser()) if checkpoint_enabled else None,
                    "resumed": checkpoint_resumed,
                    "start_nonce": checkpoint_start_nonce,
                    "save_interval": checkpoint_save_interval if checkpoint_enabled else None,
                    "cat_asset_cache_entries": len(cat_asset_cache),
                    "parent_lineage_cache_entries": len(parent_lineage_cache),
                    "last_synced_height": checkpoint_synced_height,
                },
                "scan_batches": {
                    "nonce_batch_size": nonce_batch_size,
                    "empty_batch_stop_count": empty_batch_stop_count,
                    "parent_lookup_batch_size": parent_lookup_batch_size,
                },
                "scan_window": {
                    "start_height": effective_start_height,
                    "end_height": effective_end_height,
                    "chain_peak_height": chain_peak_height,
                    "incremental_from_checkpoint": bool(args.incremental_from_checkpoint),
                    "auto_increment": bool(args.auto_increment),
                },
                "scan_stop_reason": stop_reason,
                "coins": [
                    {
                        "coin_id": row.coin_id,
                        "type": row.coin_type,
                        "cat_asset_id": row.cat_asset_id,
                        "cat_symbols": row.cat_symbols,
                        "amount": row.amount,
                        "confirmed_block_index": row.confirmed_block_index,
                        "spent_block_index": row.spent_block_index,
                        "discovered_nonces": row.discovered_nonces,
                        "discovered_by_puzzle_hash": row.discovered_by_puzzle_hash,
                        "discovered_by_hint": row.discovered_by_hint,
                        "puzzle_hash": row.puzzle_hash,
                        "parent_coin_info": row.parent_coin_info,
                    }
                    for row in filtered
                ],
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
