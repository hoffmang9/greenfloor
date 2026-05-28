#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde::Serialize;

    use crate::offer::CreateOfferRequest;
    use crate::test_support::simulator::offer_roundtrips::{
        export_offer_fixture, export_offer_leg_fixture, OfferLegScenario, OfferRoundtripScenario,
        SignerFixtureRuntimeParity,
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
        runtime_parity: SignerFixtureRuntimeParity,
        create_offer_request: CreateOfferRequest,
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
            let built = export_offer_fixture(scenario).await;
            write_fixture(out, scenario.name(), &built);
        }
        for scenario in [OfferLegScenario::BuySideDirect, OfferLegScenario::CatCatDirect] {
            let built = export_offer_leg_fixture(scenario).await;
            write_fixture(out, scenario.name(), &built);
        }
    }

    fn write_fixture(
        out: &Path,
        scenario_name: &str,
        built: &crate::test_support::simulator::offer_roundtrips::OfferBuildExport,
    ) {
        let fixture = OfferFixture {
            scenario: scenario_name.to_string(),
            execution_mode: built.result.execution_mode.to_string(),
            offer: built.result.offer.clone(),
            spend_bundle_hex: built.result.spend_bundle_hex.clone(),
            offer_nonce: built.result.offer_nonce.clone(),
            selected_coin_ids: built.result.selected_coin_ids.clone(),
            split_spend_bundle_hex: built.result.split_spend_bundle_hex.clone(),
            presplit_coin_id: built.result.presplit_coin_id.clone(),
            runtime_parity: built.runtime_parity.clone(),
            create_offer_request: built.request.clone(),
        };
        let path = out.join(format!("{scenario_name}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap()).expect("write");
    }
}
