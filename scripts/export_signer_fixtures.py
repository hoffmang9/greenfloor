#!/usr/bin/env python3
"""Export Rust simulator offer fixtures into tests/fixtures/signer/."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
FIXTURE_DIR = REPO / "tests" / "fixtures" / "signer"
EXPORT_RS = """
use std::path::Path;
use serde::Serialize;
use greenfloor_signer::offer::CreateOfferResult;
use greenfloor_signer::test_support::simulator::offer_roundtrips::{
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

#[tokio::main]
async fn main() {
    let out = Path::new("../tests/fixtures/signer");
    std::fs::create_dir_all(out).expect("mkdir");
    let scenarios = [
        OfferRoundtripScenario::Direct,
        OfferRoundtripScenario::PresplitNew { broadcast_split: false },
        OfferRoundtripScenario::PresplitExisting,
    ];
    for scenario in scenarios {
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
"""


def main() -> int:
    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    src_dir = REPO / "greenfloor-signer" / "tools"
    src_dir.mkdir(exist_ok=True)
    src_path = src_dir / "export_fixtures.rs"
    src_path.write_text(EXPORT_RS.strip() + "\n", encoding="utf-8")
    # Use cargo test export_offer_fixture indirectly via a small runner
    code = """
#[tokio::test]
async fn export_signer_fixtures_to_disk() {
    use std::path::Path;
    use serde::Serialize;
    use greenfloor_signer::test_support::simulator::offer_roundtrips::{
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
    let out = Path::new("../tests/fixtures/signer");
    std::fs::create_dir_all(out).expect("mkdir");
    for scenario in [
        OfferRoundtripScenario::Direct,
        OfferRoundtripScenario::PresplitNew { broadcast_split: false },
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
"""
    test_path = REPO / "greenfloor-signer" / "src" / "test_support" / "export_fixtures_test.rs"
    mod_path = REPO / "greenfloor-signer" / "src" / "test_support" / "mod.rs"
    if "export_fixtures_test" not in mod_path.read_text(encoding="utf-8"):
        mod_path.write_text(
            mod_path.read_text(encoding="utf-8") + "\npub mod export_fixtures_test;\n",
            encoding="utf-8",
        )
    test_path.write_text(f"#[cfg(test)]\nmod tests {{\n{code}\n}}\n", encoding="utf-8")
    subprocess.run(
        ["cargo", "test", "export_signer_fixtures_to_disk", "--", "--nocapture"],
        cwd=REPO / "greenfloor-signer",
        check=True,
    )
    print(f"exported fixtures to {FIXTURE_DIR}")
    for path in sorted(FIXTURE_DIR.glob("*.json")):
        print(path.name, json.loads(path.read_text())["execution_mode"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
