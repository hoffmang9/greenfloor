#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde::Serialize;

    use crate::test_support::simulator::offer_roundtrips::{
        export_offer_fixture, OfferRoundtripScenario,
    };

    #[derive(Serialize)]
    struct OfferFixture {
        scenario: String,
        execution_mode: String,
        offer: String,
        spend_bundle_hex: String,
        offer_nonce: String,
        selected_coin_ids: Vec<String>,
        split_spend_bundle_hex: Option<String>,
        presplit_coin_id: Option<String>,
    }

    #[tokio::test]
    async fn export_signer_fixtures_to_disk() {
        if std::env::var("EXPORT_SIGNER_FIXTURES").is_err() {
            return;
        }
        let out = Path::new("../tests/fixtures/signer");
        std::fs::create_dir_all(out).expect("mkdir");
        for scenario in [
            OfferRoundtripScenario::Direct,
            OfferRoundtripScenario::PresplitNew {
                broadcast_split: false,
            },
            OfferRoundtripScenario::PresplitExisting,
        ] {
            let result = export_offer_fixture(scenario).await;
            let fixture = OfferFixture {
                scenario: scenario.name().to_string(),
                execution_mode: result.execution_mode.to_string(),
                offer: result.offer,
                spend_bundle_hex: result.spend_bundle_hex,
                offer_nonce: result.offer_nonce,
                selected_coin_ids: result.selected_coin_ids,
                split_spend_bundle_hex: result.split_spend_bundle_hex,
                presplit_coin_id: result.presplit_coin_id,
            };
            let path = out.join(format!("{}.json", fixture.scenario));
            std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap()).expect("write");
        }
    }
}
