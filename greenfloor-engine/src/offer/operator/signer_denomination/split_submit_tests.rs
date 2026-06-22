use super::submit_bootstrap_mixed_split;
use crate::offer::bootstrap::BootstrapPlan;
use crate::test_support::signer_config::test_signer_config;

#[tokio::test]
async fn submit_bootstrap_mixed_split_rejects_invalid_asset_hex() {
    let signer = test_signer_config("https://example.test");
    let plan = BootstrapPlan {
        source_coin_id: "aa".repeat(64),
        source_amount: 1_000,
        output_amounts_base_units: vec![100],
        total_output_amount: 100,
        change_amount: 900,
        deficits: Vec::new(),
    };

    let err = submit_bootstrap_mixed_split(
        &signer,
        &plan,
        "not-a-valid-asset-id",
        "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
    )
    .await
    .expect_err("invalid asset hex");

    assert!(err.to_string().contains("hex"));
}
