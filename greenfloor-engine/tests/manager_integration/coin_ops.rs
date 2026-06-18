use super::fixtures::{run_manager, write_manager_program_with_signer, write_markets_with_ladder};

#[test]
fn coin_split_until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires --size-base-units"));
}

#[test]
fn coin_split_until_ready_disallows_no_wait() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-split",
            "--market-id",
            "m1",
            "--until-ready",
            "--size-base-units",
            "10",
            "--no-wait",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires wait mode"));
}

#[test]
fn coin_combine_until_ready_requires_size_base_units() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_manager_program_with_signer(&program, dir.path());
    write_markets_with_ladder(&markets);
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--markets-config",
            markets.to_str().expect("markets"),
            "coin-combine",
            "--market-id",
            "m1",
            "--until-ready",
            "--network",
            "mainnet",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("until-ready mode requires --size-base-units"));
}
