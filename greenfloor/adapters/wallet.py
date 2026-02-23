from __future__ import annotations

import importlib
import json
import os
import shlex
import subprocess
from dataclasses import dataclass
from pathlib import Path

from greenfloor.adapters.coinset import CoinsetAdapter
from greenfloor.core.coin_ops import CoinOpPlan
from greenfloor.keys.onboarding import load_key_onboarding_selection


@dataclass(frozen=True, slots=True)
class CoinOpExecutionItem:
    op_type: str
    size_base_units: int
    op_count: int
    status: str
    reason: str
    operation_id: str | None


class WalletAdapter:
    def execute_coin_ops(
        self,
        *,
        plans: list[CoinOpPlan],
        dry_run: bool,
        key_id: str,
        network: str,
        market_id: str | None = None,
        asset_id: str | None = None,
        receive_address: str | None = None,
        onboarding_selection_path: Path | None = None,
        signer_fingerprint: int | None = None,
    ) -> dict:
        selection_path = (
            onboarding_selection_path
            if onboarding_selection_path is not None
            else Path(".greenfloor/state/key_onboarding.json")
        )
        selection = load_key_onboarding_selection(selection_path)

        fail_ops = {
            s.strip()
            for s in os.getenv("GREENFLOOR_FAKE_COIN_OP_FAIL_TYPES", "").split(",")
            if s.strip()
        }
        items: list[CoinOpExecutionItem] = []
        executed = 0
        for idx, plan in enumerate(plans):
            if plan.op_type in fail_ops:
                items.append(
                    CoinOpExecutionItem(
                        op_type=plan.op_type,
                        size_base_units=plan.size_base_units,
                        op_count=plan.op_count,
                        status="skipped",
                        reason="simulated_failure",
                        operation_id=None,
                    )
                )
                continue
            if dry_run:
                items.append(
                    CoinOpExecutionItem(
                        op_type=plan.op_type,
                        size_base_units=plan.size_base_units,
                        op_count=plan.op_count,
                        status="planned",
                        reason=(
                            f"dry_run:{selection.selected_source}"
                            if selection is not None
                            else "dry_run:no_signer_selection"
                        ),
                        operation_id=f"dryrun-{idx}",
                    )
                )
                continue

            if selection is None:
                items.append(
                    CoinOpExecutionItem(
                        op_type=plan.op_type,
                        size_base_units=plan.size_base_units,
                        op_count=plan.op_count,
                        status="skipped",
                        reason="missing_signer_selection",
                        operation_id=None,
                    )
                )
                continue
            if selection.key_id != key_id:
                items.append(
                    CoinOpExecutionItem(
                        op_type=plan.op_type,
                        size_base_units=plan.size_base_units,
                        op_count=plan.op_count,
                        status="skipped",
                        reason="signer_key_mismatch",
                        operation_id=None,
                    )
                )
                continue
            if selection.network != network:
                items.append(
                    CoinOpExecutionItem(
                        op_type=plan.op_type,
                        size_base_units=plan.size_base_units,
                        op_count=plan.op_count,
                        status="skipped",
                        reason="signer_network_mismatch",
                        operation_id=None,
                    )
                )
                continue

            execution_item = self._execute_plan(
                plan=plan,
                selection=selection,
                key_id=key_id,
                network=network,
                market_id=market_id,
                asset_id=asset_id,
                receive_address=receive_address,
                signer_fingerprint=signer_fingerprint,
            )
            if execution_item.status == "executed":
                executed += 1
            items.append(execution_item)
        return {
            "dry_run": dry_run,
            "planned_count": len(plans),
            "executed_count": executed,
            "status": "planned_only" if dry_run else "signer_routed",
            "signer_selection": {
                "selected_source": selection.selected_source,
                "key_id": selection.key_id,
                "network": selection.network,
            }
            if selection is not None
            else None,
            "items": [
                {
                    "op_type": i.op_type,
                    "size_base_units": i.size_base_units,
                    "op_count": i.op_count,
                    "status": i.status,
                    "reason": i.reason,
                    "operation_id": i.operation_id,
                }
                for i in items
            ],
        }

    def _execute_plan(
        self,
        *,
        plan: CoinOpPlan,
        selection,
        key_id: str,
        network: str,
        market_id: str | None,
        asset_id: str | None,
        receive_address: str | None,
        signer_fingerprint: int | None,
    ) -> CoinOpExecutionItem:
        payload = {
            "key_id": key_id,
            "network": network,
            "receive_address": receive_address,
            "keyring_yaml_path": selection.keyring_yaml_path,
            "asset_id": asset_id,
            "plan": {
                "op_type": plan.op_type,
                "size_base_units": plan.size_base_units,
                "op_count": plan.op_count,
                "reason": plan.reason,
            },
        }

        if signer_fingerprint is not None:
            payload["key_id_fingerprint_map"] = {str(key_id): str(int(signer_fingerprint))}

        # External executor override (subprocess escape hatch for operators)
        cmd_raw = os.getenv("GREENFLOOR_WALLET_EXECUTOR_CMD", "").strip()
        if cmd_raw:
            return self._execute_via_subprocess(cmd_raw, payload, plan)

        # Default: direct in-process signing + broadcast
        from greenfloor.signing import sign_and_broadcast

        result = sign_and_broadcast(payload)
        status = str(result.get("status", "skipped")).strip()
        return CoinOpExecutionItem(
            op_type=plan.op_type,
            size_base_units=plan.size_base_units,
            op_count=plan.op_count,
            status=status if status in {"executed", "skipped"} else "executed",
            reason=str(result.get("reason", "unknown")),
            operation_id=result.get("operation_id"),
        )

    def _execute_via_subprocess(
        self,
        cmd_raw: str,
        payload: dict,
        plan: CoinOpPlan,
    ) -> CoinOpExecutionItem:
        try:
            completed = subprocess.run(
                shlex.split(cmd_raw),
                input=json.dumps(payload),
                capture_output=True,
                check=False,
                text=True,
                timeout=120,
            )
        except Exception as exc:
            return CoinOpExecutionItem(
                op_type=plan.op_type,
                size_base_units=plan.size_base_units,
                op_count=plan.op_count,
                status="skipped",
                reason=f"executor_spawn_error:{exc}",
                operation_id=None,
            )

        if completed.returncode != 0:
            err = completed.stderr.strip() or completed.stdout.strip() or "unknown_error"
            return CoinOpExecutionItem(
                op_type=plan.op_type,
                size_base_units=plan.size_base_units,
                op_count=plan.op_count,
                status="skipped",
                reason=f"executor_failed:{err}",
                operation_id=None,
            )

        try:
            body = json.loads(completed.stdout.strip() or "{}")
        except json.JSONDecodeError:
            return CoinOpExecutionItem(
                op_type=plan.op_type,
                size_base_units=plan.size_base_units,
                op_count=plan.op_count,
                status="skipped",
                reason="executor_invalid_json",
                operation_id=None,
            )

        status = str(body.get("status", "executed")).strip()
        reason = str(body.get("reason", "executor_success")).strip()
        operation_id = body.get("operation_id")
        return CoinOpExecutionItem(
            op_type=plan.op_type,
            size_base_units=plan.size_base_units,
            op_count=plan.op_count,
            status=status if status in {"executed", "skipped"} else "executed",
            reason=reason,
            operation_id=str(operation_id) if operation_id is not None else None,
        )

    def list_asset_coins_base_units(
        self,
        *,
        asset_id: str,
        key_id: str,
        receive_address: str,
        network: str,
    ) -> list[int]:
        _ = key_id
        raw = os.getenv("GREENFLOOR_FAKE_COINS_JSON", "").strip()
        if raw:
            fake = self._list_fake_coin_amounts(raw=raw, asset_id=asset_id)
            if fake:
                return fake

        if not self._is_xch_asset(asset_id):
            cat_raw = os.getenv("GREENFLOOR_FAKE_CAT_COINS_JSON", "").strip()
            if cat_raw:
                return self._list_fake_coin_amounts(raw=cat_raw, asset_id=asset_id)
            return []

        return self._list_coin_amounts_via_wallet_sdk(
            asset_id=asset_id,
            receive_address=receive_address,
            network=network,
        )

    @staticmethod
    def _is_xch_asset(asset_id: str) -> bool:
        lowered = asset_id.strip().lower()
        return lowered in {"xch", "1", ""}

    def _list_fake_coin_amounts(self, *, raw: str, asset_id: str) -> list[int]:
        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            return []
        if not isinstance(data, dict):
            return []
        values = data.get(asset_id, [])
        if not isinstance(values, list):
            return []
        out: list[int] = []
        for value in values:
            try:
                out.append(int(value))
            except (TypeError, ValueError):
                continue
        return out

    def _list_coin_amounts_via_wallet_sdk(
        self,
        *,
        asset_id: str,
        receive_address: str,
        network: str,
    ) -> list[int]:
        _ = asset_id
        try:
            sdk = importlib.import_module("chia_wallet_sdk")
        except Exception:
            return []

        try:
            if not hasattr(sdk, "Address"):
                return []
            address = sdk.Address.decode(receive_address)
            puzzle_hash = address.puzzle_hash
            base_url = os.getenv("GREENFLOOR_COINSET_BASE_URL", "").strip()
            coinset = CoinsetAdapter(base_url or None, network=network)
            records = coinset.get_coin_records_by_puzzle_hash(
                puzzle_hash_hex=f"0x{bytes(puzzle_hash).hex()}",
                include_spent_coins=False,
            )
            out: list[int] = []
            for record in records:
                coin_data = record.get("coin")
                if not isinstance(coin_data, dict):
                    continue
                amount = coin_data.get("amount")
                if amount is None:
                    continue
                out.append(int(amount))
            return out
        except Exception:
            return []
