use super::cli::CoinsetProbeCliArgs;
use super::report::build_coinset_probe_report;
use crate::cli_util::print_json_value;
use crate::error::{SignerError, SignerResult};

/// Probe Coinset height-window API support for vault scans and emit a JSON report.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_coinset_probe_command(args: CoinsetProbeCliArgs) -> SignerResult<()> {
    let json = args.json;
    let report = build_coinset_probe_report(args).await?;

    if json {
        print_json_value(
            &serde_json::to_value(&report)
                .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?,
            true,
        )?;
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|err| SignerError::Other(format!("json encode failed: {err}")))?
        );
    }
    Ok(())
}
