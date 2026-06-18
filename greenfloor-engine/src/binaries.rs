//! Resolve built GreenFloor native binaries for scripts and integration tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{SignerError, SignerResult};

pub const ENGINE_BIN: &str = "greenfloor-engine";
pub const MANAGER_BIN: &str = "greenfloor-manager";
pub const DAEMON_BIN: &str = "greenfloord";

const ALL_BINS: [&str; 3] = [ENGINE_BIN, MANAGER_BIN, DAEMON_BIN];

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn candidate_paths(binary_name: &str) -> [PathBuf; 4] {
    let root = repo_root();
    [
        root.join("target/debug").join(binary_name),
        root.join("target/release").join(binary_name),
        root.join("greenfloor-engine/target/debug")
            .join(binary_name),
        root.join("greenfloor-engine/target/release")
            .join(binary_name),
    ]
}

fn build_engine_binaries() -> SignerResult<()> {
    let root = repo_root();
    let manifest = root.join("greenfloor-engine/Cargo.toml");
    if !manifest.is_file() {
        return Err(SignerError::Other(
            "greenfloor-engine Cargo.toml not found; cannot build binaries".to_string(),
        ));
    }
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(&manifest)
        .current_dir(&root);
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        command.env("CARGO_TARGET_DIR", target_dir);
    } else {
        command.env("CARGO_TARGET_DIR", root.join("target"));
    }
    for binary_name in ALL_BINS {
        command.arg("--bin").arg(binary_name);
    }
    let status = command
        .status()
        .map_err(|err| SignerError::Other(format!("cargo build failed to start: {err}")))?;
    if !status.success() {
        return Err(SignerError::Other("cargo build failed".to_string()));
    }
    Ok(())
}

pub fn resolve_binary(
    binary_name: &str,
    env_var: &str,
    build_if_missing: bool,
) -> SignerResult<PathBuf> {
    if let Ok(override_path) = std::env::var(env_var) {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            let path = crate::paths::expand_home(trimmed);
            if !path.is_file() {
                return Err(SignerError::Other(format!(
                    "{env_var} is not an executable file: {}",
                    path.display()
                )));
            }
            return Ok(path);
        }
    }

    for candidate in candidate_paths(binary_name) {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Ok(path) = which::which(binary_name) {
        return Ok(path);
    }

    if build_if_missing {
        build_engine_binaries()?;
        return resolve_binary(binary_name, env_var, false);
    }

    Err(SignerError::Other(format!(
        "{binary_name} binary not found; build with \
         'cargo build --manifest-path greenfloor-engine/Cargo.toml \
         --bin greenfloor-engine --bin greenfloor-manager --bin greenfloord' \
         or set {env_var}"
    )))
}

pub fn resolve_greenfloor_engine_binary(build_if_missing: bool) -> SignerResult<PathBuf> {
    resolve_binary(ENGINE_BIN, "GREENFLOOR_ENGINE_BIN", build_if_missing)
}

pub fn resolve_greenfloor_manager_binary(build_if_missing: bool) -> SignerResult<PathBuf> {
    resolve_binary(MANAGER_BIN, "GREENFLOOR_MANAGER_BIN", build_if_missing)
}

pub fn resolve_greenfloord_binary(build_if_missing: bool) -> SignerResult<PathBuf> {
    resolve_binary(DAEMON_BIN, "GREENFLOOR_DAEMON_BIN", build_if_missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_contains_engine_manifest() {
        let root = repo_root();
        assert!(root.join("greenfloor-engine/Cargo.toml").is_file());
        assert!(root.join("pyproject.toml").is_file());
    }

    #[test]
    fn resolve_binary_honors_env_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_bin = dir.path().join(ENGINE_BIN);
        std::fs::write(&fake_bin, b"#!/bin/sh\n").expect("write fake bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake bin");
        }
        std::env::set_var("GREENFLOOR_ENGINE_BIN", fake_bin.display().to_string());
        let resolved = resolve_greenfloor_engine_binary(false).expect("resolve override");
        assert_eq!(resolved, fake_bin);
        std::env::remove_var("GREENFLOOR_ENGINE_BIN");
    }

    #[test]
    fn resolve_binary_rejects_missing_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing-greenfloord");
        std::env::set_var("GREENFLOOR_DAEMON_BIN", missing.display().to_string());
        let err = resolve_greenfloord_binary(false).expect_err("missing override");
        assert!(err.to_string().contains("GREENFLOOR_DAEMON_BIN"));
        std::env::remove_var("GREENFLOOR_DAEMON_BIN");
    }
}
