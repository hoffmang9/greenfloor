use clap::{Args, Subcommand};
use serde_json::json;

use crate::cli_util::print_json_value;
use crate::error::SignerResult;
use crate::hex;

#[derive(Debug, Args)]
pub struct HexCliArgs {
    #[command(subcommand)]
    pub command: HexCommands,
}

#[derive(Debug, Subcommand)]
pub enum HexCommands {
    #[command(name = "normalize")]
    Normalize(HexNormalizeArgs),
    #[command(name = "is-id")]
    IsId(HexIsIdArgs),
    #[command(name = "normalize-batch")]
    NormalizeBatch(HexNormalizeBatchArgs),
    #[command(name = "default-mojo-multiplier")]
    DefaultMojoMultiplier(HexDefaultMojoArgs),
}

#[derive(Debug, Args)]
pub struct HexNormalizeArgs {
    #[arg(long)]
    pub value: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HexIsIdArgs {
    #[arg(long)]
    pub value: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HexNormalizeBatchArgs {
    #[arg(long)]
    pub values_json: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct HexDefaultMojoArgs {
    #[arg(long)]
    pub asset_id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn run_hex_command(args: HexCliArgs) -> SignerResult<()> {
    match args.command {
        HexCommands::Normalize(args) => {
            let normalized = hex::normalize_hex_id(&args.value);
            if args.json {
                print_json_value(&json!({ "normalized": normalized }), true)?;
            } else {
                println!("{normalized}");
            }
        }
        HexCommands::IsId(args) => {
            let is_hex_id = hex::is_hex_id(&args.value);
            if args.json {
                print_json_value(&json!({ "is_hex_id": is_hex_id }), true)?;
            } else {
                println!("{is_hex_id}");
            }
        }
        HexCommands::NormalizeBatch(args) => {
            let values: Vec<String> = serde_json::from_str(&args.values_json).map_err(|err| {
                crate::error::SignerError::Other(format!("parse values json: {err}"))
            })?;
            let normalized: Vec<String> = values
                .iter()
                .map(|value| hex::normalize_hex_id(value))
                .collect();
            if args.json {
                print_json_value(&json!({ "normalized": normalized }), true)?;
            } else {
                for value in normalized {
                    println!("{value}");
                }
            }
        }
        HexCommands::DefaultMojoMultiplier(args) => {
            let multiplier = hex::default_mojo_multiplier_for_asset(&args.asset_id);
            if args.json {
                print_json_value(&json!({ "multiplier": multiplier }), true)?;
            } else {
                println!("{multiplier}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: HexCommands,
    }

    #[test]
    fn parses_hex_normalize_batch() {
        let cli = TestCli::try_parse_from([
            "test",
            "normalize-batch",
            "--values-json",
            r#"["0xabc"]"#,
            "--json",
        ])
        .expect("parse hex normalize-batch");
        match cli.command {
            HexCommands::NormalizeBatch(args) => {
                assert_eq!(args.values_json, r#"["0xabc"]"#);
                assert!(args.json);
            }
            _ => panic!("unexpected subcommand"),
        }
    }
}
