"""Golden fixture tests for the Rust signer (wiring + validate_offer)."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

FIXTURE_DIR = Path(__file__).resolve().parent / "fixtures" / "signer"


def _require_native():
    try:
        import greenfloor_native  # type: ignore
    except ImportError:
        pytest.skip("greenfloor_native not installed")
    return greenfloor_native


def _require_signer():
    try:
        import greenfloor_signer  # type: ignore
    except ImportError:
        pytest.skip("greenfloor_signer not installed")
    return greenfloor_signer


def _fixture_paths() -> list[Path]:
    if not FIXTURE_DIR.is_dir():
        pytest.skip(f"missing fixture directory: {FIXTURE_DIR}")
    paths = sorted(FIXTURE_DIR.glob("*.json"))
    if not paths:
        pytest.skip(f"no fixtures in {FIXTURE_DIR}")
    return paths


@pytest.mark.parametrize("fixture_path", _fixture_paths(), ids=lambda p: p.name)
def test_signer_golden_offer_validates(fixture_path: Path) -> None:
    native = _require_native()
    payload = json.loads(fixture_path.read_text(encoding="utf-8"))
    offer = str(payload.get("offer", "")).strip()
    assert offer.startswith("offer1")
    native.validate_offer(offer)


def test_signer_config_yaml_roundtrip(tmp_path: Path) -> None:
    signer = _require_signer()
    program = tmp_path / "program.yaml"
    program.write_text(
        """
app:
  network: testnet11
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/abc
  kms_region: us-west-2
  kms_public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
  coinset_msp_base_url: https://api-msp.coinset.org
vault:
  launcher_id: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58"
      curve: BLS12_381
""".strip(),
        encoding="utf-8",
    )
    context = signer.resolve_vault_context(str(program))  # type: ignore[attr-defined]
    assert context["launcher_id"] == "aa" * 32
    assert context["custody_threshold"] == 1
