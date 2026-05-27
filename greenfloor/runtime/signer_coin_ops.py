"""Vault signer coin-ops: coinset listing and mixed-split via ``greenfloor_signer``."""

from __future__ import annotations

import importlib
from typing import Any

from greenfloor.adapters import rust_signer
from greenfloor.config.models import ProgramConfig, prepare_signer_runtime
from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin
from greenfloor.runtime.offer_runtime import (
    _list_coinset_bootstrap_coins,
    _wait_for_coinset_confirmation,
)


def signer_config_path(program: ProgramConfig) -> str:
    return prepare_signer_runtime(program)


def resolve_signer_asset_id(
    program: ProgramConfig,
    *,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
) -> str:
    _ = symbol_hint
    base = str(canonical_asset_id).strip()
    if canonical_is_xch(base):
        return "xch"
    config_path = signer_config_path(program)
    resolved = rust_signer.resolve_offer_asset_ids(config_path, base, "xch")
    return str(resolved["base_asset_id"]).strip().lower()


def list_signer_asset_coins(
    *,
    program: ProgramConfig,
    receive_address: str,
    asset_id: str,
) -> list[dict[str, Any]]:
    return _list_coinset_bootstrap_coins(
        network=str(program.app_network),
        receive_address=receive_address,
        asset_id=asset_id,
    )


def filter_signer_spendable_coins(
    coins: list[dict[str, Any]],
    *,
    canonical_asset_id: str,
    min_coin_amount_mojos: int,
) -> list[dict[str, Any]]:
    min_amount = max(
        int(min_coin_amount_mojos),
        int(coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)),
    )
    return [
        coin
        for coin in coins
        if is_spendable_coin(coin) and int(coin.get("amount", 0)) >= min_amount
    ]


def resolve_hex_coin_ids(
    wallet_coins: list[dict[str, Any]], raw_coin_ids: list[str]
) -> tuple[list[str], list[str]]:
    mapping: dict[str, str] = {}
    for coin in wallet_coins:
        coin_id = str(coin.get("id", coin.get("name", ""))).strip().lower()
        if coin_id.startswith("0x"):
            coin_id = coin_id[2:]
        if coin_id:
            mapping[coin_id] = coin_id
            name = str(coin.get("name", "")).strip().lower()
            if name.startswith("0x"):
                name = name[2:]
            if name:
                mapping[name] = coin_id
    resolved: list[str] = []
    unresolved: list[str] = []
    for raw in raw_coin_ids:
        token = str(raw).strip().lower()
        if token.startswith("0x"):
            token = token[2:]
        mapped = mapping.get(token)
        if mapped:
            resolved.append(mapped)
        else:
            unresolved.append(token)
    return resolved, unresolved


def _operation_id_from_spend_bundle_hex(spend_bundle_hex: str) -> str | None:
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        spend_bundle = sdk.SpendBundle.from_bytes(bytes.fromhex(raw_hex))
        return str(sdk.to_hex(spend_bundle.hash()))
    except Exception:
        return None


def execute_signer_mixed_split(
    *,
    program: ProgramConfig,
    receive_address: str,
    asset_id: str,
    output_amounts: list[int],
    coin_ids: list[str],
    allow_sub_cat_output: bool,
    no_wait: bool,
    wait_timeout_seconds: int = 15 * 60,
    initial_coin_ids: set[str] | None = None,
) -> dict[str, Any]:
    config_path = signer_config_path(program)
    request: dict[str, Any] = {
        "receive_address": receive_address,
        "asset_id": asset_id.removeprefix("0x"),
        "output_amounts": output_amounts,
        "coin_ids": [value.removeprefix("0x") for value in coin_ids],
        "allow_sub_cat_output": allow_sub_cat_output,
        "fee_mojos": 0,
        "broadcast": True,
    }
    result = rust_signer.build_mixed_split(config_path, request)
    spend_bundle_hex = str(result.get("spend_bundle_hex", "")).strip()
    operation_id = _operation_id_from_spend_bundle_hex(spend_bundle_hex)
    payload: dict[str, Any] = {
        "spend_bundle_hex": spend_bundle_hex,
        "broadcast_status": str(result.get("broadcast_status", "")).strip() or "submitted",
        "operation_id": operation_id,
        "selected_coin_ids": result.get("selected_coin_ids", []),
        "waited": False,
        "wait_events": [],
    }
    if no_wait or initial_coin_ids is None:
        return payload
    try:
        payload["wait_events"] = _wait_for_coinset_confirmation(
            network=str(program.app_network),
            receive_address=receive_address,
            asset_id=asset_id,
            initial_coin_ids=initial_coin_ids,
            timeout_seconds=max(10, int(wait_timeout_seconds)),
        )
        payload["waited"] = True
    except Exception as exc:
        payload["wait_error"] = str(exc)
    return payload
