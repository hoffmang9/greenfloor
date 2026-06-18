from __future__ import annotations

import shutil
from pathlib import Path

from tests.helpers.manager_cli import parse_json_output, run_manager

_MNEMONIC_12 = " ".join(f"word{i}" for i in range(1, 13))


def _copy_program_config(tmp_path: Path) -> Path:
    program = tmp_path / "program.yaml"
    shutil.copyfile("config/program.yaml", program)
    return program


def test_keys_onboard_import_words_records_selection(tmp_path: Path) -> None:
    program = _copy_program_config(tmp_path)
    state_dir = tmp_path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    no_keys_dir = tmp_path / "no-keys"
    no_keys_dir.mkdir(parents=True, exist_ok=True)

    code, stdout, _stderr = run_manager(
        [
            "--program-config",
            str(program),
            "keys-onboard",
            "--key-id",
            "key-main-1",
            "--state-dir",
            str(state_dir),
            "--chia-keys-dir",
            str(no_keys_dir),
        ],
        stdin=f"1\n{_MNEMONIC_12}\n",
    )

    assert code == 0
    payload = parse_json_output(stdout)
    assert payload["selected_source"] == "mnemonic_import"
    assert payload["mnemonic_word_count"] == 12
    assert (state_dir / "key_onboarding.json").exists()


def test_keys_onboard_import_words_rejects_non_12_or_24_word_secret(tmp_path: Path) -> None:
    program = _copy_program_config(tmp_path)
    state_dir = tmp_path / "state"
    state_dir.mkdir(parents=True, exist_ok=True)
    no_keys_dir = tmp_path / "no-keys"
    no_keys_dir.mkdir(parents=True, exist_ok=True)

    code, _stdout, stderr = run_manager(
        [
            "--program-config",
            str(program),
            "keys-onboard",
            "--key-id",
            "key-main-1",
            "--state-dir",
            str(state_dir),
            "--chia-keys-dir",
            str(no_keys_dir),
        ],
        stdin="1\nnot enough words\n",
    )

    assert code == 1
    assert "mnemonic must contain 12 or 24 words" in stderr
