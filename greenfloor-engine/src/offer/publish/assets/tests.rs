use super::{
    dexie_offer_asset_expectation_error, expected_publish_asset_fields, ExpectedPublishAssetFields,
    PublishAssetSide,
};
use serde_json::json;

#[test]
fn expected_publish_fields_for_buy_side() {
    let expected = expected_publish_asset_fields("buy", "A1", "xch", "base", "quote");
    assert_eq!(
        expected,
        ExpectedPublishAssetFields {
            offered: PublishAssetSide {
                asset_id: "quote".to_string(),
                symbol: "xch".to_string(),
            },
            requested: PublishAssetSide {
                asset_id: "base".to_string(),
                symbol: "A1".to_string(),
            },
        }
    );
}

#[test]
fn expected_publish_fields_for_non_buy_side_defaults_to_sell() {
    let expected = expected_publish_asset_fields("anything_else", "A1", "xch", "base", "quote");
    assert_eq!(
        expected,
        ExpectedPublishAssetFields {
            offered: PublishAssetSide {
                asset_id: "base".to_string(),
                symbol: "A1".to_string(),
            },
            requested: PublishAssetSide {
                asset_id: "quote".to_string(),
                symbol: "xch".to_string(),
            },
        }
    );
}

#[test]
fn offered_asset_matches_by_id() {
    let offered = json!([{"id": "ABCD"}]);
    let requested = json!([]);
    let expected = ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: "abcd".to_string(),
            symbol: String::new(),
        },
        requested: PublishAssetSide {
            asset_id: String::new(),
            symbol: String::new(),
        },
    };
    assert_eq!(
        dexie_offer_asset_expectation_error(&offered, &requested, &expected),
        None
    );
}

#[test]
fn offered_asset_matches_by_code_or_name() {
    let offered = json!([{"code": "XCH"}, {"name": "txch"}]);
    let requested = json!([]);
    for symbol in ["xch", "txch"] {
        let expected = ExpectedPublishAssetFields {
            offered: PublishAssetSide {
                asset_id: "ff".to_string(),
                symbol: symbol.to_string(),
            },
            requested: PublishAssetSide {
                asset_id: String::new(),
                symbol: String::new(),
            },
        };
        assert_eq!(
            dexie_offer_asset_expectation_error(&offered, &requested, &expected),
            None
        );
    }
}

#[test]
fn error_symbol_is_lowercased_in_offered_missing_message() {
    let offered = json!([{"id": "aaaa"}]);
    let requested = json!([]);
    let expected = ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: "bbbb".to_string(),
            symbol: "B".to_string(),
        },
        requested: PublishAssetSide {
            asset_id: String::new(),
            symbol: String::new(),
        },
    };
    assert_eq!(
        dexie_offer_asset_expectation_error(&offered, &requested, &expected),
        Some("dexie_offer_offered_asset_missing:expected_asset=bbbb:expected_symbol=b".to_string())
    );
}

#[test]
fn returns_offered_error_when_expected_asset_missing() {
    let offered = json!([{"id": "aaaa"}]);
    let requested = json!([]);
    let expected = ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: "bbbb".to_string(),
            symbol: "b".to_string(),
        },
        requested: PublishAssetSide {
            asset_id: String::new(),
            symbol: String::new(),
        },
    };
    assert_eq!(
        dexie_offer_asset_expectation_error(&offered, &requested, &expected),
        Some("dexie_offer_offered_asset_missing:expected_asset=bbbb:expected_symbol=b".to_string())
    );
}

#[test]
fn returns_requested_error_when_expected_asset_missing() {
    let offered = json!([]);
    let requested = json!([{"id": "xch"}]);
    let expected = ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: String::new(),
            symbol: String::new(),
        },
        requested: PublishAssetSide {
            asset_id: "cat".to_string(),
            symbol: "cat".to_string(),
        },
    };
    assert_eq!(
        dexie_offer_asset_expectation_error(&offered, &requested, &expected),
        Some(
            "dexie_offer_requested_asset_missing:expected_asset=cat:expected_symbol=cat"
                .to_string()
        )
    );
}

#[test]
fn skips_validation_when_payload_side_is_not_a_list() {
    let offered = json!({"id": "xch"});
    let requested = json!({"id": "cat"});
    let expected = ExpectedPublishAssetFields {
        offered: PublishAssetSide {
            asset_id: "xch".to_string(),
            symbol: "xch".to_string(),
        },
        requested: PublishAssetSide {
            asset_id: "cat".to_string(),
            symbol: "cat".to_string(),
        },
    };
    assert_eq!(
        dexie_offer_asset_expectation_error(&offered, &requested, &expected),
        None
    );
}
