from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class ChiaKeysDiscovery:
    chia_keys_dir: Path
    keyring_yaml_path: Path
    has_existing_keys: bool


@dataclass(frozen=True, slots=True)
class KeyOnboardingSelection:
    selected_source: str
    key_id: str
    network: str
    chia_keys_dir: str | None = None
    keyring_yaml_path: str | None = None
    mnemonic_word_count: int | None = None


def discover_chia_keys(chia_keys_dir: Path | None = None) -> ChiaKeysDiscovery:
    base_dir = (chia_keys_dir or (Path.home() / ".chia_keys")).expanduser()
    keyring_yaml_path = base_dir / "keyring.yaml"
    has_existing_keys = keyring_yaml_path.exists()
    return ChiaKeysDiscovery(
        chia_keys_dir=base_dir,
        keyring_yaml_path=keyring_yaml_path,
        has_existing_keys=has_existing_keys,
    )


def determine_onboarding_branch(
    *,
    has_existing_keys: bool,
    use_existing_keys: bool | None,
    fallback_choice: str | None,
) -> str:
    if has_existing_keys:
        if use_existing_keys is None:
            return "prompt_use_existing_keys"
        if use_existing_keys:
            return "use_chia_keys"

    if fallback_choice is None:
        return "prompt_fallback_choice"
    if fallback_choice not in {"import_words", "generate_new"}:
        raise ValueError(f"unsupported fallback choice: {fallback_choice}")
    return fallback_choice


def save_key_onboarding_selection(path: Path, selection: KeyOnboardingSelection) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "selected_source": selection.selected_source,
        "key_id": selection.key_id,
        "network": selection.network,
        "chia_keys_dir": selection.chia_keys_dir,
        "keyring_yaml_path": selection.keyring_yaml_path,
        "mnemonic_word_count": selection.mnemonic_word_count,
    }
    path.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    return path


def load_key_onboarding_selection(path: Path) -> KeyOnboardingSelection | None:
    if not path.exists():
        return None
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(raw, dict):
        return None
    selected_source = str(raw.get("selected_source", "")).strip()
    key_id = str(raw.get("key_id", "")).strip()
    network = str(raw.get("network", "")).strip()
    if not selected_source or not key_id or not network:
        return None
    return KeyOnboardingSelection(
        selected_source=selected_source,
        key_id=key_id,
        network=network,
        chia_keys_dir=(
            str(raw["chia_keys_dir"]).strip() if raw.get("chia_keys_dir") is not None else None
        ),
        keyring_yaml_path=(
            str(raw["keyring_yaml_path"]).strip()
            if raw.get("keyring_yaml_path") is not None
            else None
        ),
        mnemonic_word_count=(
            int(raw["mnemonic_word_count"]) if raw.get("mnemonic_word_count") is not None else None
        ),
    )
