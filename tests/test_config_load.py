from pathlib import Path

from greenfloor.config.io import load_markets_config, load_program_config


def test_load_program_config() -> None:
    cfg = load_program_config(Path("config/program.yaml"))
    assert cfg.python_min_version == "3.11"
    assert cfg.low_inventory_enabled is True
    assert "key-main-1" in cfg.signer_key_registry
    assert cfg.signer_key_registry["key-main-1"].fingerprint == 123456789


def test_load_markets_config() -> None:
    cfg = load_markets_config(Path("config/markets.yaml"))
    assert len(cfg.markets) >= 2
    assert all(m.signer_key_id for m in cfg.markets if m.enabled)
