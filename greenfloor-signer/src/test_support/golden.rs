pub const LAUNCHER_ID_HEX: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const CUSTODY_KEY_HEX: &str =
    "020202020202020202020202020202020202020202020202020202020202020202";
pub const RECOVERY_KEY_HEX: &str = "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb6363b2d726218135b25b814f94df4749fc58";
pub const INNER_PUZZLE_HASH_HEX: &str =
    "c0c282903488033a205e05e42546471e140d3d2c29099588465d0e93c5a11902";
pub const P2_SINGLETON_MESSAGE_HASH_HEX: &str =
    "4141f038995622a43f2d567b8011c43819c81085066b143d942e990b8036cf6c";
pub const CUSTODY_HASH_HEX: &str =
    "a0b54784e43c1a53dac6ff8855b28741470df65399a9a6cafbb80c046e4c487c";
pub const RECOVERY_HASH_HEX: &str =
    "dcea66a7f4d21d7dfa01b5c8d4cdf1d7df4c53d3b0532ba03f0dd0ecab629107";

// Keep in sync with tests/fixtures/vault_hash_golden.json.
pub fn golden_snapshot() -> crate::vault::context::VaultCustodySnapshot {
    use crate::vault::members::{hex_to_bytes32, WalletKey};

    crate::vault::context::VaultCustodySnapshot {
        launcher_id: hex_to_bytes32(LAUNCHER_ID_HEX).expect("launcher id"),
        custody_threshold: 1,
        recovery_threshold: 1,
        recovery_clawback_timelock: 3600,
        custody_keys: vec![WalletKey {
            public_key_hex: CUSTODY_KEY_HEX.to_string(),
            curve: "SECP256R1".to_string(),
        }],
        recovery_keys: vec![WalletKey {
            public_key_hex: RECOVERY_KEY_HEX.to_string(),
            curve: "BLS12_381".to_string(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn golden_constants_match_shared_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/vault_hash_golden.json"
        ))
        .expect("fixture json");
        assert_eq!(LAUNCHER_ID_HEX, fixture["launcher_id"].as_str().unwrap());
        assert_eq!(CUSTODY_KEY_HEX, fixture["custody_key"].as_str().unwrap());
        assert_eq!(RECOVERY_KEY_HEX, fixture["recovery_key"].as_str().unwrap());
        assert_eq!(
            INNER_PUZZLE_HASH_HEX,
            fixture["inner_puzzle_hash"].as_str().unwrap()
        );
        assert_eq!(
            P2_SINGLETON_MESSAGE_HASH_HEX,
            fixture["p2_singleton_message_hash"].as_str().unwrap()
        );
        assert_eq!(CUSTODY_HASH_HEX, fixture["custody_hash"].as_str().unwrap());
        assert_eq!(
            RECOVERY_HASH_HEX,
            fixture["recovery_hash"].as_str().unwrap()
        );
    }
}
