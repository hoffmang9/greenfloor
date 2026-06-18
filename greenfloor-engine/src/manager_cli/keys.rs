//! Key onboarding for the manager CLI.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};

use super::json::ManagerOutput;
use super::paths::expand_home;

#[derive(Debug, Clone)]
struct ChiaKeysDiscovery {
    chia_keys_dir: PathBuf,
    keyring_yaml_path: PathBuf,
    has_existing_keys: bool,
}

pub fn run_keys_onboard(
    output: &ManagerOutput,
    program_path: &Path,
    key_id: &str,
    state_dir: &Path,
    chia_keys_dir: Option<&Path>,
) -> SignerResult<i32> {
    if key_id.trim().is_empty() {
        return Err(SignerError::Other("key_id must be provided".to_string()));
    }
    let program = load_program_config(program_path)?;
    let discovery = discover_chia_keys(chia_keys_dir);
    let mut use_existing_keys = false;
    if discovery.has_existing_keys {
        let prompt = format!(
            "Found existing Chia keys at '{}'. Use these keys? [Y/n]: ",
            discovery.chia_keys_dir.display()
        );
        let answer = prompt_line(&prompt)?;
        use_existing_keys = prefers_existing_chia_keys(&answer);
    }
    if discovery.has_existing_keys && use_existing_keys {
        let selection_path = save_key_onboarding_selection(
            &expand_home(state_dir).join("key_onboarding.json"),
            &json!({
                "selected_source": "chia_keys",
                "key_id": key_id.trim(),
                "network": program.network,
                "chia_keys_dir": discovery.chia_keys_dir.display().to_string(),
                "keyring_yaml_path": discovery.keyring_yaml_path.display().to_string(),
                "mnemonic_word_count": Value::Null,
            }),
        )?;
        output.emit_json(&json!({
            "selected_source": "chia_keys",
            "key_id": key_id.trim(),
            "network": program.network,
            "chia_keys_dir": discovery.chia_keys_dir.display().to_string(),
            "keyring_yaml_path": discovery.keyring_yaml_path.display().to_string(),
            "selection_path": selection_path.display().to_string(),
            "next": "unlock_on_demand",
        }))?;
        return Ok(0);
    }
    let choice = prompt_line(
        "No Chia keyring selected. Choose key onboarding path: [1] add existing words, [2] generate new key: ",
    )?;
    let fallback = match choice.trim() {
        "1" | "import_words" => "import_words",
        "2" | "generate_new" => "generate_new",
        other => {
            return Err(SignerError::Other(format!(
                "unsupported fallback choice: {other}"
            )));
        }
    };
    if fallback == "import_words" {
        let mnemonic = prompt_line("Enter existing mnemonic words: ")?;
        let words: Vec<_> = mnemonic.split_whitespace().collect();
        if words.len() != 12 && words.len() != 24 {
            return Err(SignerError::Other(
                "mnemonic must contain 12 or 24 words".to_string(),
            ));
        }
        let selection_path = save_key_onboarding_selection(
            &expand_home(state_dir).join("key_onboarding.json"),
            &json!({
                "selected_source": "mnemonic_import",
                "key_id": key_id.trim(),
                "network": program.network,
                "mnemonic_word_count": words.len(),
            }),
        )?;
        output.emit_json(&json!({
            "selected_source": "mnemonic_import",
            "key_id": key_id.trim(),
            "network": program.network,
            "mnemonic_word_count": words.len(),
            "selection_path": selection_path.display().to_string(),
            "next": "store_in_secret_manager_then_set_key_id_mapping",
        }))?;
        return Ok(0);
    }
    let selection_path = save_key_onboarding_selection(
        &expand_home(state_dir).join("key_onboarding.json"),
        &json!({
            "selected_source": "generate_new_key",
            "key_id": key_id.trim(),
            "network": program.network,
            "mnemonic_word_count": Value::Null,
        }),
    )?;
    output.emit_json(&json!({
        "selected_source": "generate_new_key",
        "key_id": key_id.trim(),
        "network": program.network,
        "selection_path": selection_path.display().to_string(),
        "next": "generate_and_store_with_wallet_sdk_key_provider",
    }))?;
    Ok(0)
}

fn discover_chia_keys(chia_keys_dir: Option<&Path>) -> ChiaKeysDiscovery {
    let base_dir = chia_keys_dir
        .map(expand_home)
        .unwrap_or_else(|| expand_home(Path::new("~/.chia_keys")));
    let keyring_yaml_path = base_dir.join("keyring.yaml");
    ChiaKeysDiscovery {
        has_existing_keys: keyring_yaml_path.exists(),
        chia_keys_dir: base_dir,
        keyring_yaml_path,
    }
}

fn save_key_onboarding_selection(path: &Path, payload: &serde_json::Value) -> SignerResult<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            SignerError::Other(format!("failed to create {}: {err}", parent.display()))
        })?;
    }
    let text = serde_json::to_string(payload)
        .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?;
    std::fs::write(path, text).map_err(|err| {
        SignerError::Other(format!("failed to write {}: {err}", path.display()))
    })?;
    Ok(path.to_path_buf())
}

fn prompt_line(prompt: &str) -> SignerResult<String> {
    eprint!("{prompt}");
    io::stderr()
        .flush()
        .map_err(|err| SignerError::Other(format!("stderr flush failed: {err}")))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| SignerError::Other(format!("stdin read failed: {err}")))?;
    Ok(line.trim().to_string())
}

fn prefers_existing_chia_keys(answer: &str) -> bool {
    let answer = answer.trim().to_ascii_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn prefers_existing_chia_keys_defaults_and_yes_variants() {
        assert!(prefers_existing_chia_keys(""));
        assert!(prefers_existing_chia_keys("Y"));
        assert!(prefers_existing_chia_keys("yes"));
        assert!(!prefers_existing_chia_keys("n"));
        assert!(!prefers_existing_chia_keys("no"));
    }

    #[test]
    fn discover_chia_keys_detects_keyring_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let keyring = dir.path().join("keyring.yaml");
        fs::write(&keyring, "keys: []\n").expect("write keyring");
        let discovery = discover_chia_keys(Some(dir.path()));
        assert!(discovery.has_existing_keys);
        assert_eq!(discovery.keyring_yaml_path, keyring);
    }

    #[test]
    fn discover_chia_keys_handles_missing_keyring_yaml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let discovery = discover_chia_keys(Some(dir.path()));
        assert!(!discovery.has_existing_keys);
    }

    #[test]
    fn save_key_onboarding_selection_writes_json_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("key_onboarding.json");
        let payload = json!({
            "selected_source": "chia_keys",
            "key_id": "key-main-1",
            "network": "mainnet",
        });
        let written = save_key_onboarding_selection(&path, &payload).expect("save");
        assert_eq!(written, path);
        let text = fs::read_to_string(&path).expect("read");
        let loaded: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            loaded.get("selected_source").and_then(Value::as_str),
            Some("chia_keys")
        );
        assert_eq!(
            loaded.get("key_id").and_then(Value::as_str),
            Some("key-main-1")
        );
    }
}
