use clap::Parser;

use crate::coinset_cli::CoinsetCommands;

#[derive(Debug, Parser)]
struct TestCli {
    #[command(subcommand)]
    command: CoinsetCommands,
}

#[test]
fn parses_nested_coinset_post_with_json() {
    let cli = TestCli::try_parse_from([
        "test",
        "post",
        "--endpoint",
        "get_all_mempool_tx_ids",
        "--body-json",
        "{}",
        "--json",
    ])
    .expect("parse coinset post");
    match cli.command {
        CoinsetCommands::Post(args) => {
            assert_eq!(args.endpoint, "get_all_mempool_tx_ids");
            assert_eq!(args.body_json, "{}");
            assert!(args.json);
        }
        _ => panic!("unexpected subcommand"),
    }
}

#[test]
fn parses_nested_coinset_push_tx() {
    let cli = TestCli::try_parse_from([
        "test",
        "push-tx",
        "--spend-bundle-hex",
        "deadbeef",
        "--json",
    ])
    .expect("parse coinset push-tx");
    match cli.command {
        CoinsetCommands::PushTx(args) => {
            assert_eq!(args.spend_bundle_hex, "deadbeef");
            assert!(args.json);
        }
        _ => panic!("unexpected subcommand"),
    }
}

#[test]
fn parses_coinset_coin_id_from_record() {
    let cli = TestCli::try_parse_from([
        "test",
        "coin-id-from-record",
        "--record-json",
        r#"{"coin":{"amount":1}}"#,
        "--json",
    ])
    .expect("parse coinset coin-id-from-record");
    match cli.command {
        CoinsetCommands::CoinIdFromRecord(args) => {
            assert_eq!(args.record_json, r#"{"coin":{"amount":1}}"#);
            assert!(args.json);
        }
        _ => panic!("unexpected subcommand"),
    }
}

#[test]
fn parses_coinset_coin_records_with_heights() {
    let cli = TestCli::try_parse_from([
        "test",
        "coin-records",
        "--endpoint",
        "get_coin_records_by_puzzle_hash",
        "--body-json",
        r#"{"puzzle_hash":"0x01"}"#,
        "--start-height",
        "10",
        "--end-height",
        "20",
        "--json",
    ])
    .expect("parse coinset coin-records");
    match cli.command {
        CoinsetCommands::CoinRecords(args) => {
            assert_eq!(args.endpoint, "get_coin_records_by_puzzle_hash");
            assert_eq!(args.start_height, Some(10));
            assert_eq!(args.end_height, Some(20));
            assert!(args.json);
        }
        _ => panic!("unexpected subcommand"),
    }
}

#[test]
fn parses_nested_coinset_probe_defaults() {
    let cli = TestCli::try_parse_from([
        "test",
        "probe",
        "--launcher-id",
        &"ab".repeat(32),
        "--height-window",
        "1000",
    ])
    .expect("parse coinset probe");
    match cli.command {
        CoinsetCommands::Probe(args) => {
            assert_eq!(args.height_window, 1000);
            assert_eq!(args.launcher_id.len(), 64);
        }
        _ => panic!("unexpected subcommand"),
    }
}
