use chia_protocol::Bytes32;
use clvm_utils::{tree_hash_pair, TreeHash};
use serde::Serialize;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::vault::members::{
    bytes32_to_hex, force_1_of_2_restriction, hex_to_bytes32, m_of_n_hash, member_hash_for_key,
    prevent_vault_side_effects_restriction, singleton_member_hash, timelock_restriction,
    tree_hash_nil, tree_hash_to_hex, MemberConfig, WalletKey,
};

#[derive(Debug, Clone)]
pub struct VaultCustodySnapshot {
    pub launcher_id: Bytes32,
    pub custody_threshold: u32,
    pub recovery_threshold: u32,
    pub recovery_clawback_timelock: u64,
    pub custody_keys: Vec<WalletKey>,
    pub recovery_keys: Vec<WalletKey>,
}

impl VaultCustodySnapshot {
    pub fn from_graphql(value: &Value) -> SignerResult<Self> {
        let launcher_id = hex_to_bytes32(
            value
                .get("vaultLauncherId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .map_err(|_| SignerError::VaultLauncherIdInvalid)?;

        let custody_threshold = parse_u32_field(value, "custodyThreshold")
            .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;
        let recovery_threshold = parse_u32_field(value, "recoveryThreshold")
            .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;
        let recovery_clawback_timelock = value
            .get("recoveryClawbackTimelock")
            .and_then(parse_json_u64)
            .ok_or(SignerError::VaultThresholdOrTimelockInvalid)?;

        let custody_keys = extract_wallet_keys(value.get("custodyKeys"));
        let recovery_keys = extract_wallet_keys(value.get("recoveryKeys"));
        if custody_keys.is_empty() || recovery_keys.is_empty() {
            return Err(SignerError::UnsupportedVaultSignerCardinality);
        }
        let custody_threshold_usize = usize::try_from(custody_threshold)
            .map_err(|_| SignerError::UnsupportedVaultThreshold)?;
        if custody_threshold == 0 || custody_threshold_usize > custody_keys.len() {
            return Err(SignerError::UnsupportedVaultThreshold);
        }
        let recovery_threshold_usize = usize::try_from(recovery_threshold)
            .map_err(|_| SignerError::UnsupportedVaultThreshold)?;
        if recovery_threshold == 0 || recovery_threshold_usize > recovery_keys.len() {
            return Err(SignerError::UnsupportedVaultThreshold);
        }
        if recovery_clawback_timelock == 0 {
            return Err(SignerError::InvalidVaultRecoveryTimelock);
        }

        Ok(Self {
            launcher_id,
            custody_threshold,
            recovery_threshold,
            recovery_clawback_timelock,
            custody_keys,
            recovery_keys,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultContext {
    pub launcher_id: String,
    pub inner_puzzle_hash: String,
    pub p2_singleton_message_hash: String,
    pub custody_hash: String,
    pub recovery_hash: String,
    pub custody_threshold: u32,
    pub recovery_threshold: u32,
    pub recovery_clawback_timelock: u64,
    pub secp256r1_custody_keys: Vec<String>,
    pub kms_public_key_hex: String,
    pub kms_custody_key_match: bool,
    pub network: String,
}

#[derive(Debug, Clone)]
pub struct VaultComputedHashes {
    pub inner_puzzle_hash: TreeHash,
    pub p2_singleton_message_hash: TreeHash,
    pub custody_hash: TreeHash,
    pub recovery_hash: TreeHash,
}

pub fn compute_vault_hashes(snapshot: &VaultCustodySnapshot) -> SignerResult<VaultComputedHashes> {
    let member_config = MemberConfig::default();
    let mut custody_hashes = snapshot
        .custody_keys
        .iter()
        .map(|key| member_hash_for_key(&member_config, key))
        .collect::<SignerResult<Vec<_>>>()?;
    custody_hashes.sort_by_key(clvm_utils::TreeHash::to_bytes);

    let custody_hash = if custody_hashes.len() == 1 {
        custody_hashes[0]
    } else {
        m_of_n_hash(&member_config, snapshot.custody_threshold, custody_hashes)?
    };

    let timelock = timelock_restriction(snapshot.recovery_clawback_timelock);
    let member_validator_list_hash =
        Bytes32::from(tree_hash_pair(timelock.puzzle_hash, tree_hash_nil()));
    let delegated_puzzle_validator_list_hash = Bytes32::from(tree_hash_nil());
    let recovery_restrictions = {
        let mut restrictions = prevent_vault_side_effects_restriction();
        restrictions.insert(
            0,
            force_1_of_2_restriction(
                Bytes32::from(custody_hash),
                0,
                member_validator_list_hash,
                delegated_puzzle_validator_list_hash,
            )?,
        );
        restrictions
    };
    let recovery_config = member_config.with_restrictions(recovery_restrictions);

    let mut recovery_hashes = snapshot
        .recovery_keys
        .iter()
        .map(|key| member_hash_for_key(&member_config, key))
        .collect::<SignerResult<Vec<_>>>()?;
    recovery_hashes.sort_by_key(clvm_utils::TreeHash::to_bytes);

    let recovery_hash = if recovery_hashes.len() == 1 {
        member_hash_for_key(&recovery_config, &snapshot.recovery_keys[0])?
    } else {
        m_of_n_hash(
            &recovery_config,
            snapshot.recovery_threshold,
            recovery_hashes,
        )?
    };

    let inner_puzzle_hash = m_of_n_hash(
        &member_config.with_top_level(true),
        1,
        vec![custody_hash, recovery_hash],
    )?;
    let p2_singleton_message_hash = singleton_member_hash(
        &MemberConfig::default().with_top_level(true),
        snapshot.launcher_id,
        false,
    );

    Ok(VaultComputedHashes {
        inner_puzzle_hash,
        p2_singleton_message_hash,
        custody_hash,
        recovery_hash,
    })
}

pub fn compute_vault_context_from_hashes(
    snapshot: &VaultCustodySnapshot,
    hashes: &VaultComputedHashes,
    kms_public_key_hex: &str,
    network: &str,
) -> SignerResult<VaultContext> {
    let secp256r1_custody_keys = snapshot
        .custody_keys
        .iter()
        .filter(|key| key.curve.trim().eq_ignore_ascii_case("SECP256R1"))
        .map(|key| crate::kms::normalize_hex(&key.public_key_hex))
        .collect::<Vec<_>>();

    let normalized_kms = crate::kms::normalize_hex(kms_public_key_hex);
    let kms_custody_key_match =
        secp256r1_custody_keys.len() == 1 && normalized_kms == secp256r1_custody_keys[0];

    if secp256r1_custody_keys.len() != 1 {
        return Err(SignerError::VaultSecp256r1KeyCount(
            secp256r1_custody_keys.len(),
        ));
    }
    if !kms_custody_key_match {
        return Err(SignerError::KmsPublicKeyMismatch {
            kms: normalized_kms,
            custody: secp256r1_custody_keys[0].clone(),
        });
    }

    Ok(VaultContext {
        launcher_id: bytes32_to_hex(snapshot.launcher_id),
        inner_puzzle_hash: tree_hash_to_hex(hashes.inner_puzzle_hash),
        p2_singleton_message_hash: tree_hash_to_hex(hashes.p2_singleton_message_hash),
        custody_hash: tree_hash_to_hex(hashes.custody_hash),
        recovery_hash: tree_hash_to_hex(hashes.recovery_hash),
        custody_threshold: snapshot.custody_threshold,
        recovery_threshold: snapshot.recovery_threshold,
        recovery_clawback_timelock: snapshot.recovery_clawback_timelock,
        secp256r1_custody_keys,
        kms_public_key_hex: normalized_kms,
        kms_custody_key_match,
        network: network.to_string(),
    })
}

pub fn compute_vault_context(
    snapshot: &VaultCustodySnapshot,
    kms_public_key_hex: &str,
    network: &str,
) -> SignerResult<VaultContext> {
    let hashes = compute_vault_hashes(snapshot)?;
    compute_vault_context_from_hashes(snapshot, &hashes, kms_public_key_hex, network)
}

fn extract_wallet_keys(connection: Option<&Value>) -> Vec<WalletKey> {
    let Some(connection) = connection else {
        return Vec::new();
    };
    let Some(edges) = connection.get("edges").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    for edge in edges {
        let Some(node) = edge.get("node").and_then(Value::as_object) else {
            continue;
        };
        let public_key = node
            .get("publicKey")
            .and_then(Value::as_str)
            .map(crate::kms::normalize_hex)
            .unwrap_or_default();
        let curve = node
            .get("curve")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        if public_key.is_empty() || curve.is_empty() {
            continue;
        }
        keys.push(WalletKey {
            public_key_hex: public_key,
            curve,
        });
    }
    keys
}

fn parse_u32_field(value: &Value, field: &str) -> Option<u32> {
    value
        .get(field)
        .and_then(parse_json_u64)
        .and_then(|v| u32::try_from(v).ok())
}

fn parse_json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
        .or_else(|| value.as_i64().and_then(|raw| u64::try_from(raw).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_snapshot_edges() {
        let snapshot = VaultCustodySnapshot::from_graphql(&json!({
            "vaultLauncherId": "aa".repeat(32),
            "custodyThreshold": 1,
            "recoveryThreshold": 1,
            "recoveryClawbackTimelock": 3600,
            "custodyKeys": {
                "edges": [{
                    "node": {
                        "publicKey": "02".repeat(33),
                        "curve": "SECP256R1"
                    }
                }]
            },
            "recoveryKeys": {
                "edges": [{
                    "node": {
                        "publicKey": "03".repeat(48),
                        "curve": "BLS12_381"
                    }
                }]
            }
        }))
        .expect("snapshot");
        assert_eq!(snapshot.custody_threshold, 1);
        assert_eq!(snapshot.custody_keys.len(), 1);
    }

    #[test]
    fn compute_vault_hashes_match_python_golden_vectors() {
        use crate::test_support::golden::{
            golden_snapshot, CUSTODY_HASH_HEX, INNER_PUZZLE_HASH_HEX,
            P2_SINGLETON_MESSAGE_HASH_HEX, RECOVERY_HASH_HEX,
        };

        let hashes = compute_vault_hashes(&golden_snapshot()).expect("hashes");
        assert_eq!(
            tree_hash_to_hex(hashes.inner_puzzle_hash),
            INNER_PUZZLE_HASH_HEX
        );
        assert_eq!(
            tree_hash_to_hex(hashes.p2_singleton_message_hash),
            P2_SINGLETON_MESSAGE_HASH_HEX
        );
        assert_eq!(tree_hash_to_hex(hashes.custody_hash), CUSTODY_HASH_HEX);
        assert_eq!(tree_hash_to_hex(hashes.recovery_hash), RECOVERY_HASH_HEX);
    }
}
