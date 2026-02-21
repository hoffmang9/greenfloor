from pathlib import Path

from greenfloor.keys.onboarding import (
    KeyOnboardingSelection,
    determine_onboarding_branch,
    discover_chia_keys,
    load_key_onboarding_selection,
    save_key_onboarding_selection,
)


def test_discover_chia_keys_detects_keyring_yaml(tmp_path: Path) -> None:
    keys_dir = tmp_path / ".chia_keys"
    keys_dir.mkdir(parents=True, exist_ok=True)
    (keys_dir / "keyring.yaml").write_text("version: 1\n", encoding="utf-8")

    discovery = discover_chia_keys(keys_dir)

    assert discovery.has_existing_keys is True
    assert discovery.chia_keys_dir == keys_dir


def test_discover_chia_keys_handles_missing_keyring_yaml(tmp_path: Path) -> None:
    keys_dir = tmp_path / ".chia_keys"
    keys_dir.mkdir(parents=True, exist_ok=True)

    discovery = discover_chia_keys(keys_dir)

    assert discovery.has_existing_keys is False


def test_determine_onboarding_branch_prefers_prompt_when_keys_exist() -> None:
    branch = determine_onboarding_branch(
        has_existing_keys=True,
        use_existing_keys=None,
        fallback_choice=None,
    )
    assert branch == "prompt_use_existing_keys"


def test_determine_onboarding_branch_uses_existing_when_confirmed() -> None:
    branch = determine_onboarding_branch(
        has_existing_keys=True,
        use_existing_keys=True,
        fallback_choice=None,
    )
    assert branch == "use_chia_keys"


def test_determine_onboarding_branch_uses_fallback_choice() -> None:
    branch = determine_onboarding_branch(
        has_existing_keys=True,
        use_existing_keys=False,
        fallback_choice="import_words",
    )
    assert branch == "import_words"


def test_save_and_load_key_onboarding_selection(tmp_path: Path) -> None:
    selection = KeyOnboardingSelection(
        selected_source="chia_keys",
        key_id="k1",
        network="testnet11",
        chia_keys_dir="/tmp/.chia_keys",
        keyring_yaml_path="/tmp/.chia_keys/keyring.yaml",
    )
    path = save_key_onboarding_selection(tmp_path / "selection.json", selection)
    loaded = load_key_onboarding_selection(path)

    assert loaded is not None
    assert loaded.selected_source == "chia_keys"
    assert loaded.key_id == "k1"
