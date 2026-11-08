//! Working one claim: translate while heartbeating, then record the outcome.

use std::path::Path;

use argo_db_client::RustChunk;

use super::Worker;
use crate::attempt::{Outcome, translate};
use crate::claim::Claim;
use crate::model::Model;
use crate::prompt;
use crate::validate::Validator;

impl<M: Model, V: Validator> Worker<M, V> {
    /// Translate and record one claim. Errs only when the model endpoint stayed unavailable:
    /// the claim is then left to lapse rather than failed, and the worker stops.
    pub(super) async fn work(&self, row: RustChunk, checkout: Option<&Path>) -> Result<(), String> {
        let claim = Claim {
            db: &self.db,
            chunk: row.chunk,
            generation: row.generation,
        };
        let id = claim.chunk;
        let Some(chunk) = self.db.chunk(id) else {
            // Release it visibly rather than hold it for a whole lease.
            let log = format!("claimed chunk {id} has no C row");
            eprintln!("argo-worker: {log}");
            let _ = claim.finish(Outcome::Failed { attempts: 0, log }).await;
            return Ok(());
        };
        eprintln!(
            "argo-worker: claimed chunk {id} `{}` (generation {})",
            chunk.name, claim.generation
        );
        let outcome = tokio::select! {
            outcome = translate(&self.model, &self.validator, &self.budget, &chunk, checkout,
                |feedback| prompt::build(&chunk, checkout, feedback)) => outcome,
            lost = claim.keep() => {
                eprintln!("argo-worker: lost chunk {id}: {lost}");
                return Ok(());
            }
        };
        let summary = match &outcome {
            Outcome::Unavailable { log } => {
                return Err(format!(
                    "the model endpoint stayed unavailable; chunk {id} is left to lapse with \
                     its lease:\n{}",
                    log.trim_end()
                ));
            }
            Outcome::Done {
                validation,
                attempts,
                ..
            } => format!("done, {validation:?}, {attempts} attempts"),
            Outcome::Failed { attempts, .. } => format!("failed after {attempts} attempts"),
        };
        match claim.finish(outcome).await {
            Ok(()) => eprintln!("argo-worker: chunk {id} {summary}"),
            Err(why) => eprintln!("argo-worker: chunk {id} {summary}, not recorded: {why}"),
        }
        Ok(())
    }
}
