//! CAS claim for expired maker rows before `PreferExisting` / reclaim I/O.

use std::future::Future;
use std::time::Duration;

use rand::RngCore;
use tokio::time::{interval, MissedTickBehavior};
use tracing::warn;

use crate::coinset::{client_for_signer_on_network, coin_id_is_unspent, LiveCoinset};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::reclaim::reclaim_presplit_maker_coin;
use crate::storage::{
    CycleWriteStore, ReusablePresplitMakerRow, MAKER_CLAIM_RENEW_INTERVAL_SECONDS,
};

use super::listing_expire::{
    finalize_maker_claim_synced, renew_maker_claim_synced, restore_maker_claim_synced,
    try_claim_expired_maker_synced,
};

/// Outcome of reclaiming one expired maker outside the ensure post path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReclaimMakerOutcome {
    Reclaimed { superseded_offer_id: String },
    Skipped,
}

fn new_maker_claim_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// CAS-won expired listing (`maker_claimed`). Restores `expired` on drop unless [`Self::commit`].
pub struct ExpiredMakerLease<'a> {
    write_store: &'a CycleWriteStore,
    market_id: String,
    offer_id: String,
    claim_token: String,
    dry_run: bool,
    committed: bool,
}

impl<'a> ExpiredMakerLease<'a> {
    /// Claim an expired row. `None` when another worker won.
    ///
    /// # Errors
    ///
    /// Returns an error when the store update fails.
    pub fn try_claim(
        write_store: &'a CycleWriteStore,
        market_id: &str,
        offer_id: &str,
        dry_run: bool,
    ) -> SignerResult<Option<Self>> {
        let claim_token = new_maker_claim_token();
        let won = try_claim_expired_maker_synced(
            write_store,
            market_id,
            offer_id,
            &claim_token,
            dry_run,
        )?;
        if !won {
            return Ok(None);
        }
        Ok(Some(Self {
            write_store,
            market_id: market_id.to_string(),
            offer_id: offer_id.to_string(),
            claim_token,
            dry_run,
            committed: false,
        }))
    }

    /// Refresh `updated_at` so stale recovery cannot steal this lease during long I/O.
    ///
    /// # Errors
    ///
    /// Returns an error when renew fails or the fence token no longer owns the claim.
    pub fn renew(&self) -> SignerResult<()> {
        let won = renew_maker_claim_synced(
            self.write_store,
            &self.market_id,
            &self.offer_id,
            &self.claim_token,
            self.dry_run,
        )?;
        if !won {
            return Err(SignerError::Other(format!(
                "maker claim renew lost fence offer_id={} market_id={}",
                self.offer_id, self.market_id
            )));
        }
        Ok(())
    }

    /// Run `fut` while periodically renewing this claim (skips heartbeat in dry-run).
    ///
    /// # Errors
    ///
    /// Returns an error when renew loses the fence or `fut` fails.
    pub async fn run_with_heartbeat<T, F>(&self, fut: F) -> SignerResult<T>
    where
        F: Future<Output = SignerResult<T>>,
    {
        // Box so `select!` does not duplicate large reclaim/post futures on the stack.
        let mut fut = Box::pin(fut);
        if self.dry_run {
            return fut.await;
        }
        let mut ticks = interval(Duration::from_secs(MAKER_CLAIM_RENEW_INTERVAL_SECONDS));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // Tokio intervals fire immediately; consume so the first renew waits a full interval.
        ticks.tick().await;
        loop {
            tokio::select! {
                result = &mut fut => return result,
                _ = ticks.tick() => {
                    self.renew()?;
                }
            }
        }
    }

    /// Finalize to `cancelled` (`PreferExisting` posted, reclaim done, or coin already spent).
    ///
    /// Disarms Drop-restore **before** finalize so a post-success / coin-spent path cannot
    /// resurrect `expired` if finalize hits a hard database error. Finalize CAS-matches
    /// the fencing token so a stale-recovered late worker cannot cancel another claim.
    ///
    /// # Errors
    ///
    /// Returns an error when finalize fails or the fence token no longer owns the claim.
    pub fn commit(mut self) -> SignerResult<()> {
        self.committed = true;
        let won = finalize_maker_claim_synced(
            self.write_store,
            &self.market_id,
            &self.offer_id,
            &self.claim_token,
            self.dry_run,
        )?;
        if !won {
            return Err(SignerError::Other(format!(
                "maker claim finalize lost fence offer_id={} market_id={}",
                self.offer_id, self.market_id
            )));
        }
        Ok(())
    }
}

impl Drop for ExpiredMakerLease<'_> {
    fn drop(&mut self) {
        if self.committed || self.dry_run {
            return;
        }
        match restore_maker_claim_synced(
            self.write_store,
            &self.market_id,
            &self.offer_id,
            &self.claim_token,
            false,
        ) {
            Ok(true) => {}
            Ok(false) => {
                warn!(
                    offer_id = %self.offer_id,
                    market_id = %self.market_id,
                    "maker_claimed restore lost fence (stale recovery or other owner)"
                );
            }
            Err(err) => {
                warn!(
                    offer_id = %self.offer_id,
                    market_id = %self.market_id,
                    error = %err,
                    "failed to restore maker_claimed → expired after failed ensure/reclaim"
                );
            }
        }
    }
}

/// Reclaim an expired maker after CAS-claiming its listing row.
///
/// Spent coins are retired via claim+finalize without a reclaim spend so they do not
/// stick in `expired` across soft-expire cycles. `Skipped` means another worker owns the row.
///
/// # Errors
///
/// Returns an error when Coinset lookup or reclaim build/broadcast fails (listing restored
/// on reclaim failure after a successful claim).
pub async fn reclaim_expired_maker_if_unspent(
    write_store: &CycleWriteStore,
    signer: SignerConfig,
    network: &str,
    row: &ReusablePresplitMakerRow,
    dry_run: bool,
) -> SignerResult<ReclaimMakerOutcome> {
    let Some(lease) =
        ExpiredMakerLease::try_claim(write_store, &row.market_id, &row.offer_id, dry_run)?
    else {
        return Ok(ReclaimMakerOutcome::Skipped);
    };
    let coinset_client = client_for_signer_on_network(&signer, network)?;
    let backend = LiveCoinset(&coinset_client);
    lease.renew()?;
    if !coin_id_is_unspent(&backend, &row.cancel_input_coin_id).await? {
        // Coin already gone (take/reclaim elsewhere): retire sticky expired without spend.
        lease.commit()?;
        return Ok(ReclaimMakerOutcome::Reclaimed {
            superseded_offer_id: row.offer_id.clone(),
        });
    }
    lease
        .run_with_heartbeat(reclaim_presplit_maker_coin(
            signer,
            network,
            &row.cancel_input_coin_id,
            &row.fixed_delegated_puzzle_hash,
            dry_run,
        ))
        .await?;
    lease.commit()?;
    Ok(ReclaimMakerOutcome::Reclaimed {
        superseded_offer_id: row.offer_id.clone(),
    })
}
