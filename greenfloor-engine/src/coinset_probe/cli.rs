use clap::Parser;

#[derive(Debug, Parser)]
pub struct CoinsetProbeCliArgs {
    #[arg(long, default_value = "mainnet")]
    pub network: String,
    #[arg(long, default_value = "")]
    pub coinset_base_url: String,
    #[arg(long, default_value = "")]
    pub launcher_id: String,
    #[arg(long, default_value = "")]
    pub launcher_id_file: String,
    #[arg(
        long,
        default_value = "",
        help = "Path to program.yaml used to resolve vault.launcher_id when --launcher-id is omitted."
    )]
    pub program_config: String,
    #[arg(long, default_value_t = 0, help = "Member nonce to probe (default 0).")]
    pub nonce: u32,
    #[arg(
        long,
        default_value_t = 50_000,
        help = "Probe range window in blocks from chain peak (default 50000)."
    )]
    pub height_window: u64,
    #[arg(long)]
    pub json: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_coinset_probe_defaults() {
        let args = CoinsetProbeCliArgs::try_parse_from(["probe"]).expect("parse");
        assert_eq!(args.network, "mainnet");
        assert_eq!(args.nonce, 0);
        assert_eq!(args.height_window, 50_000);
        assert!(!args.json);
    }
}
