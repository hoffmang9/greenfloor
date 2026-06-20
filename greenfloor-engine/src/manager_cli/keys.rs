//! Key onboarding for the manager CLI.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};

use super::context::ManagerContext;
use super::paths::expand_home;

#[derive(Debug, Clone)]
struct ChiaKeysDiscovery {
    chia_keys_dir: PathBuf,
    keyring_yaml_path: PathBuf,
    has_existing_keys: bool,
}

pub fn run_keys_onboard(
    ctx: &ManagerContext,
    key_id: &str,
    state_dir: &Path,
    chia_keys_dir: Option<&Path>,
) -> SignerResult<i32> {
    if key_id.trim().is_empty() {
        return Err(SignerError::Other("key_id must be provided".to_string()));
    }
    let program = load_program_config(&ctx.program_config)?;
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
        ctx.emit_json(&json!({
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
        ctx.emit_json(&json!({
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
    ctx.emit_json(&json!({
        "selected_source": "generate_new_key",
        "key_id": key_id.trim(),
        "network": program.network,
        "selection_path": selection_path.display().to_string(),
        "next": "generate_and_store_with_wallet_sdk_key_provider",
    }))?;
    Ok(0)
}

fn discover_chia_keys(chia_keys_dir: Option<&Path>) -> ChiaKeysDiscovery {
    let base_dir =
        chia_keys_dir.map_or_else(|| expand_home(Path::new("~/.chia_keys")), expand_home);
    let keyring_yaml_path = base_dir.join("keyring.yaml");
    ChiaKeysDiscovery {
        has_existing_keys: keyring_yaml_path.exists(),
        chia_keys_dir: base_dir,
        keyring_yaml_path,
    }
}

fn save_key_onboarding_selection(
    path: &Path,
    payload: &serde_json::Value,
) -> SignerResult<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            SignerError::Other(format!("failed to create {}: {err}", parent.display()))
        })?;
    }
    let text = serde_json::to_string(payload)
        .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?;
    std::fs::write(path, text)
        .map_err(|err| SignerError::Other(format!("failed to write {}: {err}", path.display())))?;
    Ok(path.to_path_buf())
}

fn prompt_line(prompt: &str) -> SignerResult<String> {
    #[cfg(test)]
    if let Some(line) = take_test_prompt_line() {
        return Ok(line);
    }
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
mod test_prompt {
    use std::sync::Mutex;

    pub static LINES: Mutex<Option<Vec<String>>> = Mutex::new(None);
}

#[cfg(test)]
fn take_test_prompt_line() -> Option<String> {
    let mut guard = test_prompt::LINES.lock().expect("prompt lines lock");
    let lines = guard.as_mut()?;
    if lines.is_empty() {
        return None;
    }
    Some(lines.remove(0))
}

#[cfg(test)]
struct TestPromptLines {
    _private: (),
}

#[cfg(test)]
impl TestPromptLines {
    fn new(lines: Vec<&str>) -> Self {
        *test_prompt::LINES.lock().expect("prompt lines lock") =
            Some(lines.into_iter().map(str::to_string).collect());
        Self { _private: () }
    }
}

#[cfg(test)]
impl Drop for TestPromptLines {
    fn drop(&mut self) {
        *test_prompt::LINES.lock().expect("prompt lines lock") = None;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::manager_cli::context::ManagerContext;
    use crate::manager_cli::json::ManagerOutput;

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf()
    }

    fn copy_example_program(dest: &Path) -> std::path::PathBuf {
        let program = dest.join("program.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        program
    }

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

    #[test]
    fn keys_onboard_import_words_records_selection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = copy_example_program(dir.path());
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("create state");
        let no_keys_dir = dir.path().join("no-keys");
        std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
        let mnemonic = (1..=12)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let _prompts = TestPromptLines::new(vec!["1", &mnemonic]);
        let (output, captured) = ManagerOutput::capturing(true);
        let ctx = ManagerContext::for_test_with_output(
            program,
            dir.path().join("unused-markets.yaml"),
            output,
        );
        let code = run_keys_onboard(&ctx, "key-main-1", &state_dir, Some(no_keys_dir.as_path()))
            .expect("keys-onboard");
        assert_eq!(code, 0);
        let payload = captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted");
        assert_eq!(
            payload.get("selected_source"),
            Some(&json!("mnemonic_import"))
        );
        assert_eq!(payload.get("mnemonic_word_count"), Some(&json!(12)));
        assert!(state_dir.join("key_onboarding.json").is_file());
    }

    #[test]
    fn keys_onboard_import_words_rejects_non_12_or_24_word_secret() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = copy_example_program(dir.path());
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("create state");
        let no_keys_dir = dir.path().join("no-keys");
        std::fs::create_dir_all(&no_keys_dir).expect("create no-keys");
        let _prompts = TestPromptLines::new(vec!["1", "not enough words"]);
        let ctx = ManagerContext::for_test(program, dir.path().join("unused-markets.yaml"));
        let err = run_keys_onboard(&ctx, "key-main-1", &state_dir, Some(no_keys_dir.as_path()))
            .expect_err("invalid mnemonic");
        assert!(err
            .to_string()
            .contains("mnemonic must contain 12 or 24 words"));
    }
}
