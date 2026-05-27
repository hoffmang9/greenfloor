"""CLI key onboarding commands."""

from __future__ import annotations

from pathlib import Path

from greenfloor.config.io import load_program_config
from greenfloor.keys.onboarding import (
    KeyOnboardingSelection,
    determine_onboarding_branch,
    discover_chia_keys,
    save_key_onboarding_selection,
)
from greenfloor.runtime.cloud_wallet.adapter import _format_json_output as format_json_output


def keys_onboard(
    *,
    program_path: Path,
    key_id: str,
    state_dir: Path,
    chia_keys_dir: Path | None = None,
) -> int:
    program = load_program_config(program_path)
    if not key_id.strip():
        raise ValueError("key_id must be provided")
    discovery = discover_chia_keys(chia_keys_dir)
    branch = determine_onboarding_branch(
        has_existing_keys=discovery.has_existing_keys,
        use_existing_keys=None,
        fallback_choice=None,
    )

    use_existing_keys = False
    if branch == "prompt_use_existing_keys":
        raw = (
            input(
                f"Found existing Chia keys at '{discovery.chia_keys_dir}'. Use these keys? [Y/n]: "
            )
            .strip()
            .lower()
        )
        use_existing_keys = raw in {"", "y", "yes"}
        branch = determine_onboarding_branch(
            has_existing_keys=discovery.has_existing_keys,
            use_existing_keys=use_existing_keys,
            fallback_choice=None,
        )

    if branch == "use_chia_keys":
        selection = KeyOnboardingSelection(
            selected_source="chia_keys",
            key_id=key_id,
            network=program.app_network,
            chia_keys_dir=str(discovery.chia_keys_dir),
            keyring_yaml_path=str(discovery.keyring_yaml_path),
        )
        selection_path = save_key_onboarding_selection(
            state_dir / "key_onboarding.json",
            selection,
        )
        print(
            format_json_output(
                {
                    "selected_source": "chia_keys",
                    "key_id": key_id,
                    "network": program.app_network,
                    "chia_keys_dir": str(discovery.chia_keys_dir),
                    "keyring_yaml_path": str(discovery.keyring_yaml_path),
                    "selection_path": str(selection_path),
                    "next": "unlock_on_demand",
                }
            )
        )
        return 0

    raw_choice = input(
        "No Chia keyring selected. Choose key onboarding path: [1] add existing words, [2] generate new key: "
    ).strip()
    fallback_choice = (
        "import_words" if raw_choice == "1" else "generate_new" if raw_choice == "2" else ""
    )
    if fallback_choice == "":
        raise ValueError("invalid onboarding choice; expected 1 or 2")
    branch = determine_onboarding_branch(
        has_existing_keys=discovery.has_existing_keys,
        use_existing_keys=False,
        fallback_choice=fallback_choice,
    )

    if branch == "import_words":
        mnemonic = input("Enter existing mnemonic words: ").strip()
        words = [w for w in mnemonic.split() if w]
        if len(words) not in {12, 24}:
            raise ValueError("mnemonic must contain 12 or 24 words")
        selection = KeyOnboardingSelection(
            selected_source="mnemonic_import",
            key_id=key_id,
            network=program.app_network,
            mnemonic_word_count=len(words),
        )
        selection_path = save_key_onboarding_selection(
            state_dir / "key_onboarding.json",
            selection,
        )
        print(
            format_json_output(
                {
                    "selected_source": "mnemonic_import",
                    "key_id": key_id,
                    "network": program.app_network,
                    "mnemonic_word_count": len(words),
                    "selection_path": str(selection_path),
                    "next": "store_in_secret_manager_then_set_key_id_mapping",
                }
            )
        )
        return 0

    selection = KeyOnboardingSelection(
        selected_source="generate_new_key",
        key_id=key_id,
        network=program.app_network,
    )
    selection_path = save_key_onboarding_selection(
        state_dir / "key_onboarding.json",
        selection,
    )
    print(
        format_json_output(
            {
                "selected_source": "generate_new_key",
                "key_id": key_id,
                "network": program.app_network,
                "selection_path": str(selection_path),
                "next": "generate_and_store_with_wallet_sdk_key_provider",
            }
        )
    )
    return 0
