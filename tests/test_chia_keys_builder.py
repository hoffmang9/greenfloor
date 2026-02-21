import os
import sys

from greenfloor.cli.chia_keys_builder import execute_payload


def _payload(keyring_yaml_path: str = "/tmp/.chia_keys/keyring.yaml") -> dict:
    return {
        "key_id": "fingerprint:123456789",
        "network": "testnet11",
        "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        "keyring_yaml_path": keyring_yaml_path,
        "asset_id": "xch",
        "plan": {
            "op_type": "split",
            "size_base_units": 10,
            "op_count": 2,
            "target_total_base_units": 20,
            "reason": "r",
        },
        "selected_coins": [
            {
                "coin_id": "03",
                "parent_coin_info": "01",
                "puzzle_hash": "02",
                "amount": 50,
            }
        ],
    }


def test_builder_returns_sign_request_when_bundle_signer_missing(tmp_path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    old = os.getenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD")
    try:
        os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD", None)
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "skipped"
        assert out["reason"].startswith("sdk_submit_")
        assert (
            out.get("sign_job", {}).get("plan", {}).get("target_total_base_units") == 20
            or out.get("engine_request", {}).get("plan", {}).get("target_total_base_units") == 20
            or out.get("sign_request", {}).get("plan", {}).get("target_total_base_units") == 20
        )
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD"] = old


def test_builder_delegates_to_bundle_signer(tmp_path) -> None:
    keyring = tmp_path / "keyring.yaml"
    keyring.write_text("version: 1\n", encoding="utf-8")
    script = tmp_path / "bundle_signer.py"
    script.write_text(
        (
            "import json,sys\n"
            "_ = json.loads(sys.stdin.read())\n"
            'print(json.dumps({"status":"executed","reason":"ok","spend_bundle_hex":"00ff"}))\n'
        ),
        encoding="utf-8",
    )
    old = os.getenv("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD")
    try:
        os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD"] = f"{sys.executable} {script}"
        out = execute_payload(_payload(str(keyring)))
        assert out["status"] == "executed"
        assert out["spend_bundle_hex"] == "00ff"
    finally:
        if old is None:
            os.environ.pop("GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD", None)
        else:
            os.environ["GREENFLOOR_CHIA_KEYS_BUNDLE_SIGNER_CMD"] = old
