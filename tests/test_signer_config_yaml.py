"""Config and routing tests for the Rust signer migration."""

from __future__ import annotations

from dataclasses import replace

import pytest

from greenfloor.config.models import (
    ProgramConfig,
    VaultConfig,
    VaultWalletKeyConfig,
    coin_ops_execution_backend,
    managed_offer_execution_backend,
    offer_execution_backend,
    signer_offer_path_configured,
)
from tests.helpers.config_fixtures import minimal_program_config


def _program_with_signer(*, kms_key_id: str = "arn:aws:kms:us-west-2:1:key/x") -> ProgramConfig:
    vault = VaultConfig(
        launcher_id="aa" * 32,
        custody_threshold=1,
        recovery_threshold=1,
        recovery_clawback_timelock=3600,
        custody_keys=(
            VaultWalletKeyConfig(
                public_key_hex="0202" * 32,
                curve="SECP256R1",
            ),
        ),
        recovery_keys=(
            VaultWalletKeyConfig(
                public_key_hex="ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58",
                curve="BLS12_381",
            ),
        ),
    )
    return replace(
        minimal_program_config(home_dir="/tmp/gf-test"),
        app_network="testnet11",
        runtime_loop_interval_seconds=15,
        tx_block_websocket_url="wss://testnet11.api.coinset.org/ws",
        signer_kms_key_id=kms_key_id,
        signer_kms_region="us-west-2",
        vault_config=vault,
    )


def test_signer_offer_path_requires_kms_and_vault() -> None:
    program = _program_with_signer()
    assert signer_offer_path_configured(program) is True
    program_no_vault = replace(program, vault_config=None, signer_kms_key_id="arn:x")
    assert signer_offer_path_configured(program_no_vault) is False


def test_coin_ops_execution_backend_prefers_signer() -> None:
    program = _program_with_signer()
    assert coin_ops_execution_backend(program) == "signer"


def test_offer_execution_backend_requires_signer() -> None:
    program = _program_with_signer()
    assert offer_execution_backend(program, size_base_units=50) == "signer"
    assert managed_offer_execution_backend(program, size_base_units=50) == "signer"


def test_offer_execution_backend_raises_without_signer() -> None:
    program = replace(_program_with_signer(kms_key_id=""), vault_config=None)
    with pytest.raises(ValueError, match="offer execution requires signer"):
        offer_execution_backend(program, size_base_units=50)
    with pytest.raises(ValueError, match="offer execution requires signer"):
        managed_offer_execution_backend(program, size_base_units=50)


def test_invalidate_signer_runtime_cache_clears_entries() -> None:
    from pathlib import Path

    from greenfloor.config import models
    from greenfloor.config.models import invalidate_signer_runtime_cache

    home_a = str(Path("/tmp/a").resolve())
    home_b = str(Path("/tmp/b").resolve())
    models._prepared_signer_config_by_home[home_a] = f"{home_a}/signer.yaml"
    models._prepared_signer_config_by_home[home_b] = f"{home_b}/signer.yaml"
    invalidate_signer_runtime_cache(home_dir="/tmp/a")
    assert home_a not in models._prepared_signer_config_by_home
    assert models._prepared_signer_config_by_home[home_b] == f"{home_b}/signer.yaml"
    invalidate_signer_runtime_cache()
    assert not models._prepared_signer_config_by_home
