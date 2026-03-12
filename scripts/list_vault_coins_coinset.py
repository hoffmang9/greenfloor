#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter, CloudWalletConfig
from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.hex_utils import is_hex_id, normalize_hex_id


def _import_sdk() -> Any:
    return importlib.import_module("chia_wallet_sdk")


def _hex_to_bytes(value: str) -> bytes:
    raw = value.strip().lower()
    if raw.startswith("0x"):
        raw = raw[2:]
    if len(raw) % 2:
        raw = f"0{raw}"
    return bytes.fromhex(raw)


def _to_coinset_hex(value: bytes) -> str:
    return f"0x{value.hex()}"


def _safe_int(value: object, default: int = 0) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def _coin_id_from_record(record: dict[str, Any]) -> str:
    coin = record.get("coin")
    if not isinstance(coin, dict):
        return ""
    for candidate in (
        coin.get("name"),
        coin.get("coin_id"),
        coin.get("coin_name"),
        record.get("name"),
    ):
        normalized = normalize_hex_id(candidate)
        if normalized:
            return normalized
    parent_hex = str(coin.get("parent_coin_info", "")).strip()
    puzzle_hex = str(coin.get("puzzle_hash", "")).strip()
    amount = _safe_int(coin.get("amount"), default=-1)
    if not parent_hex or not puzzle_hex or amount < 0:
        return ""
    return hashlib.sha256(
        _hex_to_bytes(parent_hex) + _hex_to_bytes(puzzle_hex) + int(amount).to_bytes(8, "big")
    ).hexdigest()


def _coin_from_record(*, sdk: Any, record: dict[str, Any]) -> Any | None:
    coin_data = record.get("coin")
    if not isinstance(coin_data, dict):
        return None
    parent_hex = str(coin_data.get("parent_coin_info", "")).strip()
    puzzle_hex = str(coin_data.get("puzzle_hash", "")).strip()
    if not parent_hex or not puzzle_hex:
        return None
    try:
        return sdk.Coin(
            _hex_to_bytes(parent_hex), _hex_to_bytes(puzzle_hex), int(coin_data.get("amount", 0))
        )
    except Exception:
        return None


@dataclass(slots=True)
class CoinRow:
    coin_id: str
    puzzle_hash: str
    parent_coin_info: str
    amount: int
    confirmed_block_index: int
    spent_block_index: int
    discovered_nonces: list[int]
    discovered_by_puzzle_hash: bool
    discovered_by_hint: bool
    coin_type: str
    cat_asset_id: str | None
    cat_symbols: list[str]


def _is_spendable_coin_state(state: str) -> bool:
    coin_state = str(state or "").strip().upper()
    return coin_state in {"CONFIRMED", "UNSPENT", "SPENDABLE", "AVAILABLE", "SETTLED"}


class CoinsetScanner:
    def __init__(self, *, network: str, base_url: str | None = None) -> None:
        require_testnet11 = network.strip().lower() in {"testnet", "testnet11"}
        self.adapter = CoinsetAdapter(
            base_url=base_url, network=network, require_testnet11=require_testnet11
        )

    def _post_json(self, endpoint: str, body: dict[str, Any]) -> dict[str, Any]:
        payload = dict(body)
        if self.adapter.network == "testnet11":
            payload.setdefault("network", "testnet11")
        req = urllib.request.Request(
            f"{self.adapter.base_url}/{endpoint}",
            data=json.dumps(payload).encode("utf-8"),
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
                "User-Agent": "greenfloor-vault-coinset-scanner/0.1",
            },
        )
        with urllib.request.urlopen(req, timeout=20) as resp:
            parsed = json.loads(resp.read().decode("utf-8"))
        if not isinstance(parsed, dict):
            raise RuntimeError("coinset_invalid_response_payload")
        return parsed

    def by_puzzle_hash(self, *, puzzle_hash: str, include_spent: bool) -> list[dict[str, Any]]:
        return self.adapter.get_coin_records_by_puzzle_hash(
            puzzle_hash_hex=puzzle_hash,
            include_spent_coins=include_spent,
        )

    def by_hint(self, *, hint: str, include_spent: bool) -> list[dict[str, Any]]:
        payload = self._post_json(
            "get_coin_records_by_hint",
            {"hint": hint, "include_spent_coins": include_spent},
        )
        if not payload.get("success", False):
            return []
        rows = payload.get("coin_records") or []
        return [row for row in rows if isinstance(row, dict)]


def _detect_cat_asset_id(
    *, sdk: Any, coinset: CoinsetScanner, record: dict[str, Any]
) -> str | None:
    coin = _coin_from_record(sdk=sdk, record=record)
    if coin is None:
        return None
    parent_record = coinset.adapter.get_coin_record_by_name(
        coin_name_hex=_to_coinset_hex(coin.parent_coin_info)
    )
    if not isinstance(parent_record, dict):
        return None
    parent_coin = _coin_from_record(sdk=sdk, record=parent_record)
    if parent_coin is None:
        return None
    spent_height = _safe_int(parent_record.get("spent_block_index"), default=0)
    if spent_height <= 0:
        return None
    solution = coinset.adapter.get_puzzle_and_solution(
        coin_id_hex=_to_coinset_hex(parent_coin.coin_id()),
        height=spent_height,
    )
    if not isinstance(solution, dict):
        return None
    puzzle_reveal_hex = str(solution.get("puzzle_reveal", "")).strip()
    solution_hex = str(solution.get("solution", "")).strip()
    if not puzzle_reveal_hex or not solution_hex:
        return None
    try:
        clvm = sdk.Clvm()
        parent_puzzle_program = clvm.deserialize(_hex_to_bytes(puzzle_reveal_hex))
        parent_solution_program = clvm.deserialize(_hex_to_bytes(solution_hex))
        parsed_children = parent_puzzle_program.puzzle().parse_child_cats(
            parent_coin, parent_solution_program
        )
    except Exception:
        return None
    if not parsed_children:
        return None
    wanted_id = sdk.to_hex(coin.coin_id())
    for cat in parsed_children:
        child_coin = getattr(cat, "coin", None)
        info = getattr(cat, "info", None)
        if child_coin is None or info is None:
            continue
        if sdk.to_hex(child_coin.coin_id()) != wanted_id:
            continue
        asset_id = normalize_hex_id(sdk.to_hex(info.asset_id))
        return asset_id or None
    return None


def _launcher_from_cloud_wallet(args: argparse.Namespace) -> str:
    wallet = CloudWalletAdapter(
        CloudWalletConfig(
            base_url=args.cloud_wallet_base_url,
            user_key_id=args.cloud_wallet_user_key_id,
            private_key_pem_path=args.cloud_wallet_private_key_pem_path,
            vault_id=args.vault_id,
            network=args.network,
        )
    )
    snapshot = wallet.get_vault_custody_snapshot()
    launcher = (
        normalize_hex_id(snapshot.get("vaultLauncherId")) if isinstance(snapshot, dict) else ""
    )
    if not launcher:
        raise RuntimeError("vault_launcher_id_missing_from_cloud_wallet_snapshot")
    return launcher


def _require_cloud_wallet_adapter(args: argparse.Namespace) -> CloudWalletAdapter:
    required = [
        args.cloud_wallet_base_url,
        args.cloud_wallet_user_key_id,
        args.cloud_wallet_private_key_pem_path,
        args.vault_id,
    ]
    if any(not str(v).strip() for v in required):
        raise ValueError(
            "combine mode requires Cloud Wallet args: "
            "--cloud-wallet-base-url, --cloud-wallet-user-key-id, "
            "--cloud-wallet-private-key-pem-path, --vault-id"
        )
    return CloudWalletAdapter(
        CloudWalletConfig(
            base_url=args.cloud_wallet_base_url,
            user_key_id=args.cloud_wallet_user_key_id,
            private_key_pem_path=args.cloud_wallet_private_key_pem_path,
            vault_id=args.vault_id,
            network=args.network,
        )
    )


def _resolve_cloud_wallet_cat_global_id(wallet: CloudWalletAdapter, cat_asset_id_hex: str) -> str:
    query = """
query resolveAssetByIdentifier($identifier: String) {
  asset(identifier: $identifier) {
    id
    type
  }
}
"""
    payload = wallet._graphql(query=query, variables={"identifier": cat_asset_id_hex})  # noqa: SLF001
    asset = payload.get("asset") if isinstance(payload, dict) else None
    if not isinstance(asset, dict):
        raise RuntimeError(f"cloud_wallet_asset_lookup_failed:{cat_asset_id_hex}")
    global_id = str(asset.get("id", "")).strip()
    asset_type = str(asset.get("type", "")).strip().upper()
    if not global_id.startswith("Asset_") or asset_type not in {"CAT", "CAT2"}:
        raise RuntimeError(f"cloud_wallet_asset_lookup_not_cat:{cat_asset_id_hex}:{asset_type}")
    return global_id


def _coin_name_to_global_id_map_for_asset(
    wallet: CloudWalletAdapter,
    *,
    asset_global_id: str,
) -> dict[str, str]:
    coins = wallet.list_coins(include_pending=True)
    mapping: dict[str, str] = {}
    for coin in coins:
        coin_global_id = str(coin.get("id", "")).strip()
        coin_name = normalize_hex_id(coin.get("name"))
        state = str(coin.get("state", "")).strip()
        if not coin_global_id or not coin_name:
            continue
        if not _is_spendable_coin_state(state):
            continue
        if bool(coin.get("isLocked", False)):
            continue
        asset = coin.get("asset")
        asset_id = str(asset.get("id", "")).strip() if isinstance(asset, dict) else ""
        if asset_id != asset_global_id:
            continue
        mapping[coin_name] = coin_global_id
    return mapping


def _combine_cat_dust(
    *,
    args: argparse.Namespace,
    wallet: CloudWalletAdapter,
    rows: list[CoinRow],
    requested_cat_ids: set[str],
) -> dict[str, Any]:
    threshold = max(1, int(args.dust_threshold_mojos))
    max_inputs = max(2, int(args.combine_max_inputs))
    fee_mojos = max(0, int(args.combine_fee_mojos))
    dry_run = bool(args.combine_dry_run)

    dust_by_asset: dict[str, list[CoinRow]] = {}
    for row in rows:
        if row.coin_type != "CAT" or not row.cat_asset_id:
            continue
        if int(row.amount) >= threshold:
            continue
        if requested_cat_ids and row.cat_asset_id not in requested_cat_ids:
            continue
        dust_by_asset.setdefault(row.cat_asset_id, []).append(row)

    operations: list[dict[str, Any]] = []
    for asset_id_hex in sorted(dust_by_asset.keys()):
        dust_rows = sorted(dust_by_asset[asset_id_hex], key=lambda c: (c.amount, c.coin_id))
        if len(dust_rows) < 2:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "status": "skipped",
                    "reason": "insufficient_dust_coins",
                    "dust_coin_count": len(dust_rows),
                }
            )
            continue
        asset_global_id = _resolve_cloud_wallet_cat_global_id(wallet, asset_id_hex)
        global_map = _coin_name_to_global_id_map_for_asset(wallet, asset_global_id=asset_global_id)
        unresolved_coin_ids = [row.coin_id for row in dust_rows if row.coin_id not in global_map]
        if unresolved_coin_ids:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "asset_global_id": asset_global_id,
                    "status": "error",
                    "reason": "coin_id_not_mappable_to_cloud_wallet_global_id",
                    "unresolved_coin_ids": unresolved_coin_ids,
                    "operator_guidance": (
                        "re-run after wallet sync settles or pass a narrower filter; Cloud Wallet must expose "
                        "CoinRecord global ids for all selected coin names before combine can be submitted"
                    ),
                }
            )
            continue

        input_coin_global_ids = [global_map[row.coin_id] for row in dust_rows]
        batch_plans: list[dict[str, Any]] = []
        for offset in range(0, len(input_coin_global_ids), max_inputs):
            batch = input_coin_global_ids[offset : offset + max_inputs]
            if len(batch) <= 1:
                continue
            batch_plans.append(
                {
                    "input_coin_ids": batch,
                    "input_coin_count": len(batch),
                }
            )

        if dry_run:
            operations.append(
                {
                    "cat_asset_id": asset_id_hex,
                    "asset_global_id": asset_global_id,
                    "status": "dry_run",
                    "fee_mojos": fee_mojos,
                    "dust_coin_count": len(dust_rows),
                    "batches": batch_plans,
                }
            )
            continue

        submitted_batches: list[dict[str, Any]] = []
        for batch in batch_plans:
            result = wallet.combine_coins(
                number_of_coins=int(batch["input_coin_count"]),
                fee=fee_mojos,
                largest_first=False,
                asset_id=asset_global_id,
                input_coin_ids=list(batch["input_coin_ids"]),
            )
            submitted_batches.append(
                {
                    **batch,
                    "signature_request_id": str(result.get("signature_request_id", "")).strip(),
                    "status": str(result.get("status", "")).strip(),
                }
            )

        operations.append(
            {
                "cat_asset_id": asset_id_hex,
                "asset_global_id": asset_global_id,
                "status": "submitted",
                "fee_mojos": fee_mojos,
                "dust_coin_count": len(dust_rows),
                "submitted_batches": submitted_batches,
            }
        )

    return {
        "threshold_mojos": threshold,
        "combine_max_inputs": max_inputs,
        "combine_fee_mojos": fee_mojos,
        "combine_dry_run": dry_run,
        "operations": operations,
    }


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


def _load_yaml_mapping(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        parsed = yaml.safe_load(handle) or {}
    if not isinstance(parsed, dict):
        return {}
    return parsed


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
            cats_payload = _load_yaml_mapping(cats_path)
        except Exception:
            cats_payload = {}
        cats = cats_payload.get("cats") if isinstance(cats_payload, dict) else None
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
            markets_payload = _load_yaml_mapping(markets_path)
        except Exception:
            markets_payload = {}
        markets = markets_payload.get("markets") if isinstance(markets_payload, dict) else None
        if isinstance(markets, list):
            for market in markets:
                if not isinstance(market, dict):
                    continue
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
        help="Optional vault launcher id hex; fetched from Cloud Wallet when omitted.",
    )
    parser.add_argument(
        "--launcher-id-file",
        default="",
        help="Read launcher id from this file when --launcher-id is omitted; fetched launcher id is saved here too.",
    )
    parser.add_argument(
        "--resolve-launcher-id-only",
        action="store_true",
        help="Resolve launcher id (from arg/file/cloud wallet), print it, then exit.",
    )
    parser.add_argument("--cloud-wallet-base-url", default="")
    parser.add_argument("--cloud-wallet-user-key-id", default="")
    parser.add_argument("--cloud-wallet-private-key-pem-path", default="")
    parser.add_argument("--vault-id", default="")
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
        "--combine-dust",
        action="store_true",
        help="Combine CAT dust coins (< threshold mojos) using explicit Cloud Wallet inputCoinIds.",
    )
    parser.add_argument(
        "--combine-dry-run",
        action="store_true",
        help="Plan CAT dust combines but do not submit Cloud Wallet combine requests.",
    )
    parser.add_argument(
        "--dust-threshold-mojos",
        type=int,
        default=1000,
        help="Dust threshold in CAT mojos; coins with amount below this are selected (default 1000).",
    )
    parser.add_argument(
        "--combine-max-inputs",
        type=int,
        default=5,
        help="Maximum input coins per combine mutation batch (default 5).",
    )
    parser.add_argument(
        "--combine-fee-mojos",
        type=int,
        default=0,
        help="Fee mojos per combine request (default 0).",
    )
    args = parser.parse_args()

    launcher_id = normalize_hex_id(args.launcher_id)
    launcher_id_source = "arg"
    if not launcher_id and str(args.launcher_id_file).strip():
        launcher_id = _read_launcher_id_file(args.launcher_id_file)
        if launcher_id:
            launcher_id_source = "file"
    if not launcher_id:
        required = [
            args.cloud_wallet_base_url,
            args.cloud_wallet_user_key_id,
            args.cloud_wallet_private_key_pem_path,
            args.vault_id,
        ]
        if any(not str(v).strip() for v in required):
            raise ValueError(
                "launcher-id, launcher-id-file, or full Cloud Wallet auth args are required"
            )
        launcher_id = _launcher_from_cloud_wallet(args)
        launcher_id_source = "cloud_wallet"
    if str(args.launcher_id_file).strip() and launcher_id_source in {"cloud_wallet", "arg"}:
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

    by_coin_id: dict[str, CoinRow] = {}
    nonce_to_p2: dict[int, str] = {}

    for nonce in range(0, max(0, int(args.max_nonce)) + 1):
        cfg = sdk.MemberConfig().with_top_level(True).with_nonce(int(nonce))
        p2_hash = normalize_hex_id(
            sdk.to_hex(sdk.singleton_member_hash(cfg, _hex_to_bytes(launcher_id), False))
        )
        if not p2_hash:
            continue
        nonce_to_p2[nonce] = p2_hash
        by_puzzle = scanner.by_puzzle_hash(
            puzzle_hash=_to_coinset_hex(_hex_to_bytes(p2_hash)), include_spent=args.include_spent
        )
        by_hint = scanner.by_hint(
            hint=_to_coinset_hex(_hex_to_bytes(p2_hash)), include_spent=args.include_spent
        )
        if nonce > 0 and not by_puzzle and not by_hint:
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
                if nonce not in row.discovered_nonces:
                    row.discovered_nonces.append(nonce)
                    row.discovered_nonces.sort()
                if source == "puzzle_hash":
                    row.discovered_by_puzzle_hash = True
                if source == "hint":
                    row.discovered_by_hint = True

    for row in by_coin_id.values():
        p2_hashes = {nonce_to_p2.get(nonce, "") for nonce in row.discovered_nonces}
        if row.puzzle_hash and row.puzzle_hash in p2_hashes:
            row.coin_type = "XCH"
            continue
        record = {
            "coin": {
                "parent_coin_info": row.parent_coin_info,
                "puzzle_hash": row.puzzle_hash,
                "amount": row.amount,
            },
        }
        cat_asset_id = _detect_cat_asset_id(sdk=sdk, coinset=scanner, record=record)
        if cat_asset_id:
            row.coin_type = "CAT"
            row.cat_asset_id = cat_asset_id
            row.cat_symbols = list(asset_id_to_symbols.get(cat_asset_id, []))
            continue
        row.coin_type = "OTHER"

    filtered: list[CoinRow] = []
    for row in sorted(by_coin_id.values(), key=lambda r: (r.coin_type, r.amount, r.coin_id)):
        if effective_asset_type == "xch" and row.coin_type != "XCH":
            continue
        if effective_asset_type == "cat" and row.coin_type != "CAT":
            continue
        if requested_cat_ids and (row.cat_asset_id or "") not in requested_cat_ids:
            continue
        filtered.append(row)

    combine_plan: dict[str, Any] | None = None
    if bool(args.combine_dust):
        wallet = _require_cloud_wallet_adapter(args)
        combine_plan = _combine_cat_dust(
            args=args,
            wallet=wallet,
            rows=filtered,
            requested_cat_ids=requested_cat_ids,
        )

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
                "count": len(filtered),
                "combine_dust": combine_plan,
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
