use chia_protocol::Bytes32;
use chia_puzzle_types::Proof;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use chia_sdk_driver::{Vault, VaultInfo};
use clvm_utils::TreeHash;

use super::parse::coin_records_from_response;
use crate::error::{SignerError, SignerResult};

/// Fetch latest vault.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn fetch_latest_vault(
    client: &CoinsetClient,
    launcher_id: Bytes32,
    inner_puzzle_hash: TreeHash,
) -> SignerResult<Vault> {
    let response = client
        .get_coin_records_by_parent_ids(vec![launcher_id], None, None, Some(true), None)
        .await
        .map_err(SignerError::from)?;
    let launcher_children = coin_records_from_response(response)?;
    let Some(first_child) = launcher_children.first() else {
        return Err(SignerError::VaultSingletonNotFound);
    };
    let singleton_puzzle_hash = first_child.coin.puzzle_hash;
    let leaf_response = client
        .get_coin_records_by_puzzle_hash(singleton_puzzle_hash, None, None, Some(false), None)
        .await
        .map_err(SignerError::from)?;
    let mut leaf_candidates = coin_records_from_response(leaf_response)?;
    if leaf_candidates.is_empty() {
        return Err(SignerError::VaultSingletonNotFound);
    }
    leaf_candidates.sort_by_key(|record| std::cmp::Reverse(record.confirmed_block_index));
    let current = &leaf_candidates[0];
    let parent_id = current.coin.parent_coin_info;
    let parent_response = client
        .get_coin_record_by_name(parent_id)
        .await
        .map_err(SignerError::from)?;
    let Some(parent_record) = parent_response.coin_record else {
        return Err(SignerError::VaultSingletonNotFound);
    };
    let parent_parent = parent_record.coin.parent_coin_info;
    let proof = if parent_id == launcher_id {
        Proof::Eve(chia_puzzle_types::EveProof {
            parent_parent_coin_info: parent_parent,
            parent_amount: parent_record.coin.amount,
        })
    } else {
        Proof::Lineage(chia_puzzle_types::LineageProof {
            parent_parent_coin_info: parent_parent,
            parent_inner_puzzle_hash: inner_puzzle_hash.into(),
            parent_amount: parent_record.coin.amount,
        })
    };
    Ok(Vault::new(
        current.coin,
        proof,
        VaultInfo::new(launcher_id, inner_puzzle_hash),
    ))
}

#[cfg(test)]
mod tests {
    use chia_protocol::Bytes32;
    use chia_sdk_coinset::CoinsetClient;
    use clvm_utils::TreeHash;

    use super::fetch_latest_vault;
    use crate::error::SignerError;

    fn hex32(byte: u8) -> String {
        format!("0x{}", hex::encode([byte; 32]))
    }

    fn coin_record_json(
        parent_coin_info: &str,
        puzzle_hash: &str,
        amount: u64,
        confirmed_block_index: u32,
    ) -> String {
        format!(
            r#"{{"coin":{{"parent_coin_info":"{parent_coin_info}","puzzle_hash":"{puzzle_hash}","amount":{amount}}},"confirmed_block_index":{confirmed_block_index},"spent":false,"spent_block_index":0,"coinbase":false,"timestamp":0}}"#
        )
    }

    #[tokio::test]
    async fn fetch_latest_vault_returns_vault_singleton_not_found_when_launcher_has_no_children() {
        let launcher_id = Bytes32::new([0x11; 32]);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_parent_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .create_async()
            .await;

        let client = CoinsetClient::new(server.url());
        let err = fetch_latest_vault(&client, launcher_id, TreeHash::new([0x22; 32]))
            .await
            .expect_err("missing launcher child");
        assert!(matches!(err, SignerError::VaultSingletonNotFound));
    }

    #[tokio::test]
    async fn fetch_latest_vault_builds_eve_proof_when_parent_is_launcher() {
        let launcher_id = Bytes32::new([0x11; 32]);
        let singleton_ph = hex32(0x22);
        let leaf_parent = hex32(0x11);
        let leaf_ph = hex32(0x33);
        let parent_parent = hex32(0x44);
        let launcher_child = coin_record_json(&hex32(0x11), &singleton_ph, 1, 1);
        let leaf = coin_record_json(&leaf_parent, &leaf_ph, 2, 100);
        let parent = coin_record_json(&parent_parent, &singleton_ph, 1, 99);

        let mut server = mockito::Server::new_async().await;
        let _parent_ids = server
            .mock("POST", "/get_coin_records_by_parent_ids")
            .with_status(200)
            .with_body(format!(
                r#"{{"success":true,"coin_records":[{launcher_child}]}}"#
            ))
            .create_async()
            .await;
        let _puzzle_hash = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(format!(r#"{{"success":true,"coin_records":[{leaf}]}}"#))
            .create_async()
            .await;
        let _parent = server
            .mock("POST", "/get_coin_record_by_name")
            .with_status(200)
            .with_body(format!(r#"{{"success":true,"coin_record":{parent}}}"#))
            .create_async()
            .await;

        let client = CoinsetClient::new(server.url());
        let vault = fetch_latest_vault(&client, launcher_id, TreeHash::new([0x55; 32]))
            .await
            .expect("vault");
        assert_eq!(vault.info.launcher_id, launcher_id);
        assert_eq!(vault.coin.amount, 2);
        assert!(matches!(vault.proof, chia_puzzle_types::Proof::Eve(_)));
    }
}
