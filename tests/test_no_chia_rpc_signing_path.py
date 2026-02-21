from pathlib import Path


def test_no_chia_rpc_wallet_usage_in_signing_pipeline() -> None:
    root = Path(__file__).resolve().parents[1]
    pipeline_files = [
        root / "greenfloor" / "cli" / "wallet_executor.py",
        root / "greenfloor" / "cli" / "chia_keys_executor.py",
        root / "greenfloor" / "cli" / "chia_keys_passthrough.py",
        root / "greenfloor" / "cli" / "chia_keys_worker.py",
        root / "greenfloor" / "cli" / "chia_keys_signer.py",
        root / "greenfloor" / "cli" / "chia_keys_signer_backend.py",
        root / "greenfloor" / "cli" / "chia_keys_builder.py",
        root / "greenfloor" / "cli" / "chia_keys_bundle_signer.py",
        root / "greenfloor" / "cli" / "chia_keys_bundle_signer_raw.py",
        root / "greenfloor" / "cli" / "chia_keys_raw_engine.py",
        root / "greenfloor" / "cli" / "chia_keys_raw_engine_sign.py",
        root / "greenfloor" / "cli" / "chia_keys_raw_engine_sign_impl.py",
        root / "greenfloor" / "cli" / "chia_keys_raw_engine_sign_impl_sdk_submit.py",
    ]
    forbidden_snippets = ["chia rpc wallet", '"chia", "rpc", "wallet"', "'chia', 'rpc', 'wallet'"]
    offenders: list[str] = []
    for file_path in pipeline_files:
        text = file_path.read_text(encoding="utf-8")
        lowered = text.lower()
        if any(snippet in lowered for snippet in forbidden_snippets):
            offenders.append(str(file_path.relative_to(root)))
    assert offenders == [], f"forbidden chia rpc signing path found in: {offenders}"
