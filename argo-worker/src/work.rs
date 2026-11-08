//! The claim loop: wait for a complete ingest, bring the clone to its revision, claim, find the
//! placeholder through the subscription, translate while heartbeating, then `submit_rust` or
//! `fail_chunk` quoting the claim's generation.

use std::collections::HashSet;
use std::time::Duration;

use argo_db_client::RustChunk;
use argo_db_client::bindings::claim_next;
use tokio::time::sleep;

use crate::agent::Budget;
use crate::checkout::Checkout;
use crate::db::{CallError, Db, WAIT};
use crate::model::Model;
use crate::select::{idle, open};
use crate::validate::Validator;

mod one;

/// Pause before claiming again when nothing could be claimed (ingest unsealed, or every open
/// chunk leased by another worker).
const IDLE: Duration = Duration::from_secs(2);
/// How long to look for a claim whose `claim_next` outcome was lost: it may have committed. One
/// that surfaces later is still worked, before the next claim ([`Worker::next`]).
const LOST: Duration = Duration::from_secs(5);

pub struct Worker<M, V> {
    pub db: Db,
    pub model: M,
    pub validator: V,
    pub budget: Budget,
    pub checkout: Checkout,
}

/// Runs on the task that calls [`Worker::run`] (`main`'s); its futures are not `Send`, so a
/// [`Validator`] needn't be either.
impl<M: Model, V: Validator> Worker<M, V> {
    /// Work until every translatable chunk is `Done` or `Failed` (or after one claim, `once`).
    /// An empty or partial ingest (before the first, or after an owner's reset) is waited out.
    pub async fn run(&mut self, once: bool) -> Result<(), String> {
        // Generations of this worker's claims already worked (or given up as lost).
        let mut handled = HashSet::new();
        loop {
            let Some(origin) = self.db.ingested()? else {
                sleep(IDLE).await;
                continue;
            };
            if !open(&self.db.chunks(), &self.db.rusts()) {
                break;
            }
            // Clone before claiming, so a git failure never strands a claim.
            self.checkout.sync(&origin)?;
            let Some(claim) = self.next(&handled).await? else {
                continue;
            };
            handled.insert(claim.generation);
            // Claims exist only under a complete ingest, and the cache holds the claim, so it
            // holds that ingest too unless a reset followed (then the claim is already lost).
            // The sync is a no-op unless a reset and re-ingest of another commit slipped in
            // before the claim; a git failure then leaves the claim to lapse with its lease.
            let checkout = match self.db.ingested() {
                Ok(Some(origin)) => self.checkout.sync(&origin)?,
                _ => {
                    eprintln!("argo-worker: chunk {} lost to a reset", claim.chunk);
                    continue;
                }
            };
            self.work(claim, checkout.as_deref()).await?;
            if once {
                return Ok(());
            }
        }
        eprintln!("argo-worker: every translatable chunk is done or failed");
        Ok(())
    }

    /// The next claim to work: one already held but not yet `handled` (a `claim_next` whose
    /// outcome was lost, surfacing late), else a new one. `None` to look again shortly.
    async fn next(&self, handled: &HashSet<u32>) -> Result<Option<RustChunk>, String> {
        if let Some(held) = self.db.claimed(handled, Duration::ZERO).await {
            return Ok(Some(held));
        }
        let within = match self.db.call(|r, f| r.claim_next_then(f)).await {
            Ok(()) => WAIT,
            Err(CallError::Rejected(why)) if idle(&why) => {
                eprintln!("argo-worker: nothing claimed ({why}); retrying");
                sleep(IDLE).await;
                return Ok(None);
            }
            Err(CallError::Rejected(why)) => return Err(format!("claim_next: {why}")),
            Err(e @ CallError::Unknown(_)) => {
                eprintln!("argo-worker: claim_next {e}; looking for the claim");
                LOST
            }
        };
        match self.db.claimed(handled, within).await {
            None if within == WAIT => Err("the claim never reached the subscription".into()),
            found => Ok(found),
        }
    }
}
