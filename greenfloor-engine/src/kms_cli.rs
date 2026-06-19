use clap::Args;

use crate::cli_util::print_json_value;
use crate::error::SignerResult;
use crate::kms;

#[derive(Debug, Args)]
pub struct KmsPublicKeyArgs {
    #[arg(long)]
    pub key_id: String,
    #[arg(long)]
    pub region: String,
    #[arg(long)]
    pub json: bool,
}

pub async fn run_kms_public_key_compressed_hex(args: KmsPublicKeyArgs) -> SignerResult<()> {
    let compressed_hex = kms::get_public_key_compressed_hex(&args.key_id, &args.region).await?;
    if args.json {
        print_json_value(
            &serde_json::json!({ "public_key_compressed_hex": compressed_hex }),
            true,
        )?;
    } else {
        println!("{compressed_hex}");
    }
    Ok(())
}
