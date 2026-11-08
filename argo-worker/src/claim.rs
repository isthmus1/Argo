//! A held claim: the chunk and fencing generation every write about it quotes. Writes after the
//! claim was lost (lease expired and reclaimed, requeued, reset) are rejected by the module.

use std::time::Duration;

use argo_db_client::LEASE_MICROS;
use argo_db_client::bindings::{RemoteReducers, fail_chunk, heartbeat, submit_rust};
use tokio::time::{Instant, sleep};

use crate::attempt::Outcome;
use crate::db::{CallError, Db};
use crate::log::{MAX_LOG_BYTES, tail};
use crate::resend::deliver;

/// Renew well inside the lease, so a slow reducer round trip never lets it lapse.
const HEARTBEAT: Duration = Duration::from_micros(LEASE_MICROS as u64 / 10);
/// How long heartbeats may go unconfirmed (outcome unknown, not rejected) before the claim is
/// given up: half a lease, so it is dropped well before another worker could reclaim it.
const PATIENCE: Duration = Duration::from_micros(LEASE_MICROS as u64 / 2);
/// How long to look for a `submit_rust` whose outcome was lost in the subscription, per send.
const CONFIRM: Duration = Duration::from_secs(5);

pub struct Claim<'a> {
    pub db: &'a Db,
    pub chunk: u64,
    pub generation: u32,
}

impl Claim<'_> {
    /// Renew the lease until the module rejects a renewal (the claim is gone) or none has been
    /// confirmed for [`PATIENCE`]; returns why. One slow round trip is not a lost claim.
    pub async fn keep(&self) -> String {
        let (id, generation) = (self.chunk, self.generation);
        let mut renewed = Instant::now();
        loop {
            sleep(HEARTBEAT).await;
            match self
                .db
                .call(|r, f| r.heartbeat_then(id, generation, f))
                .await
            {
                Ok(()) => renewed = Instant::now(),
                Err(CallError::Rejected(why)) => return why,
                Err(CallError::Unknown(why)) if renewed.elapsed() >= PATIENCE => {
                    return format!("no heartbeat confirmed for {PATIENCE:?}: {why}");
                }
                Err(CallError::Unknown(why)) => {
                    eprintln!("argo-worker: heartbeat for chunk {id}: {why}; retrying");
                }
            }
        }
    }

    /// Record the outcome: `submit_rust` for a candidate, `fail_chunk` for a give-up, nothing
    /// for an unavailable endpoint (the claim lapses with its lease).
    pub async fn finish(&self, outcome: Outcome) -> Result<(), String> {
        let (id, generation) = (self.chunk, self.generation);
        let (source, model, attempts, validation, log) = match outcome {
            Outcome::Failed { attempts, log } => return self.fail(attempts, log).await,
            Outcome::Unavailable { .. } => return Ok(()),
            Outcome::Done {
                source,
                model,
                attempts,
                validation,
                log,
            } => (source, model, attempts, validation, log),
        };
        let submit = async || {
            let (source, model, log) = (source.clone(), model.clone(), log.clone());
            let submit = |r: &RemoteReducers, f| {
                r.submit_rust_then(id, generation, source, model, attempts, validation, log, f)
            };
            self.db.call(submit).await
        };
        // A lost outcome may hide a commit; the subscription shows whether there was one.
        let landed = async || self.db.submitted(id, generation, CONFIRM).await;
        let why = match deliver(submit, landed).await {
            Ok(()) => return Ok(()),
            Err(e) => e.to_string(),
        };
        // Rejected, or never confirmed: leave the chunk visibly failed rather than leased for
        // ten more minutes, keeping the attempt history. If the claim is no longer ours this is
        // rejected too, which is then the right outcome.
        let log = tail(&format!("{log}submit_rust {why}\n"), MAX_LOG_BYTES);
        let failed = self.fail(attempts, log).await;
        Err(format!("submit_rust {why}; fail_chunk: {failed:?}"))
    }

    async fn fail(&self, attempts: u32, log: String) -> Result<(), String> {
        let (id, generation) = (self.chunk, self.generation);
        let fail = |r: &RemoteReducers, f| r.fail_chunk_then(id, generation, attempts, log, f);
        self.db.call(fail).await.map_err(|e| e.to_string())
    }
}
