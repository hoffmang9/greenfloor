from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest

from greenfloor.cli.manager import _keys_onboard


def _copy_program_config(tmp_path: Path) -> Path:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    return program


def test_keys_onboard_import_words_accepts_24_word_secret(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    program = _copy_program_config(tmp_path)
    state_dir = tmp_path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    no_keys_dir = tmp_path / "no-keys"
    no_keys_dir.mkdir(parents=True, exist_ok=True)
    mnemonic_24 = " ".join([f"word{i}" for i in range(1, 25)])
    responses = iter(["1", mnemonic_24])
    monkeypatch.setattr("builtins.input", lambda _prompt="": next(responses))

    code = _keys_onboard(
        program_path=program,
        key_id="key-main-1",
        state_dir=state_dir,
        chia_keys_dir=no_keys_dir,
    )

    assert code == 0
    payload = json.loads(capsys.readouterr().out.strip())
    assert payload["selected_source"] == "mnemonic_import"
    assert payload["mnemonic_word_count"] == 24
    assert (state_dir / "key_onboarding.json").exists()


def test_keys_onboard_import_words_rejects_non_12_or_24_word_secret(
    monkeypatch, tmp_path: Path
) -> None:
    program = _copy_program_config(tmp_path)
    state_dir = tmp_path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    no_keys_dir = tmp_path / "no-keys"
    no_keys_dir.mkdir(parents=True, exist_ok=True)
    mnemonic_23 = " ".join([f"word{i}" for i in range(1, 24)])
    responses = iter(["1", mnemonic_23])
    monkeypatch.setattr("builtins.input", lambda _prompt="": next(responses))

    with pytest.raises(ValueError, match="mnemonic must contain 12 or 24 words"):
        _keys_onboard(
            program_path=program,
            key_id="key-main-1",
            state_dir=state_dir,
            chia_keys_dir=no_keys_dir,
        )
