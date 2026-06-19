//! Coinset probe CLI integration tests.

#[test]
fn subprocess_coinset_probe_parses_defaults() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args(["coinset", "probe", "--help"])
        .output()
        .expect("spawn greenfloor-engine coinset probe --help");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--launcher-id"));
    assert!(help.contains("--height-window"));
    assert!(help.contains("--program-config"));
}

#[test]
fn subprocess_coinset_probe_requires_launcher_source() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "probe",
            "--program-config",
            "/nonexistent/program.yaml",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset probe");
    assert!(!output.status.success());
}

#[test]
fn subprocess_coinset_probe_accepts_launcher_id_arg() {
    let launcher = "ab".repeat(32);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_greenfloor-engine"))
        .args([
            "coinset",
            "probe",
            "--launcher-id",
            &launcher,
            "--coinset-base-url",
            "https://invalid.example.test",
        ])
        .output()
        .expect("spawn greenfloor-engine coinset probe with launcher id");
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("coinset") || combined.contains("error") || combined.contains("probe"),
        "unexpected output: {combined}"
    );
}
