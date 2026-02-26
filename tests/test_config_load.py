from pathlib import Path

from greenfloor.config.io import load_markets_config, load_program_config


def test_load_program_config() -> None:
    cfg = load_program_config(Path("config/program.yaml"))
    assert cfg.python_min_version == "3.11"
    assert cfg.low_inventory_enabled is True
    assert cfg.app_log_level == "INFO"
    assert cfg.tx_block_trigger_mode == "websocket"
    assert cfg.tx_block_websocket_url.startswith("wss://")
    assert "key-main-1" in cfg.signer_key_registry
    assert cfg.signer_key_registry["key-main-1"].fingerprint == 123456789


def test_load_markets_config() -> None:
    cfg = load_markets_config(Path("config/markets.yaml"))
    assert len(cfg.markets) >= 2
    assert all(m.signer_key_id for m in cfg.markets if m.enabled)


def test_load_program_config_defaults_log_level_to_info_when_missing(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO\n", "")
    config_path = tmp_path / "program-missing-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    cfg = load_program_config(config_path)
    assert cfg.app_log_level == "INFO"
    rewritten = config_path.read_text(encoding="utf-8")
    assert "log_level: INFO" in rewritten


def test_load_program_config_defaults_log_level_to_info_when_invalid(tmp_path: Path) -> None:
    source = Path("config/program.yaml").read_text(encoding="utf-8")
    candidate = source.replace("  log_level: INFO", "  log_level: totally-not-a-level")
    config_path = tmp_path / "program-invalid-log-level.yaml"
    config_path.write_text(candidate, encoding="utf-8")
    cfg = load_program_config(config_path)
    assert cfg.app_log_level == "INFO"
