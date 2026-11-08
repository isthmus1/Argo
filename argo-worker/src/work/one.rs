//! Working one claim: build the C program, run the agent while heartbeating, then record the
//! outcome.

use std::path::Path;
use std::sync::Arc;

use argo_db_client::{CChunk, ChunkKind, RustChunk};

use super::Worker;
use crate::agent::{Outcome, Task, translate};
use crate::claim::Claim;
use crate::model::Model;
use crate::validate::{Ingest, Validator};

impl<M: Model, V: Validator> Worker<M, V> {
    /// Translate and record one claim. Errs only when the model endpoint stayed unavailable or
    /// the claim's session directory can't be made: the claim is then left to lapse rather than
    /// failed, and the worker stops.
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
        // The claim's own directory: the C program and every submission, never the clone.
        // Removed with the claim.
        let session = tempfile::Builder::new()
            .prefix(&format!("argo-chunk-{id}-"))
            .tempdir()
            .map_err(|e| format!("cannot make a session directory for chunk {id}: {e}"))?;
        eprintln!(
            "argo-worker: claimed chunk {id} `{}` (generation {}, session {})",
            chunk.name,
            claim.generation,
            session.path().display()
        );
        // A piece of a function is translated on its own, inside its function.
        let piece = crate::piece::Piece::of(&chunk, self.function_of(&chunk));
        let translated = async {
            let origin = self.db.ingested().ok().flatten();
            let ingest = origin
                .as_ref()
                .zip(checkout)
                .map(|(origin, checkout)| Ingest { origin, checkout });
            let dir = session.path().join("c-program");
            let program = self
                .validator
                .prepare(&chunk, piece.as_ref(), ingest, &dir)
                .await
                .map(Arc::new);
            for note in program.iter().flat_map(|p| &p.notes) {
                eprintln!("argo-worker: chunk {id}: {note}");
            }
            let task = Task {
                chunk: &chunk,
                checkout,
                session: session.path(),
                program,
                piece: piece.as_ref(),
            };
            let agent = translate(&self.model, &self.validator, &self.budget, &task);
            crate::model::CLAIM
                .scope((id, claim.generation), agent)
                .await
        };
        let outcome = tokio::select! {
            outcome = translated => outcome,
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
            } => format!("done, {validation:?}, {attempts} submission(s)"),
            Outcome::Failed { attempts, .. } => format!("failed after {attempts} submission(s)"),
        };
        match claim.finish(outcome).await {
            Ok(()) => eprintln!("argo-worker: chunk {id} {summary}"),
            Err(why) => eprintln!("argo-worker: chunk {id} {summary}, not recorded: {why}"),
        }
        Ok(())
    }

    /// The function row a `Region` row is inside: the nearest `Function` up its parents.
    fn function_of(&self, chunk: &CChunk) -> Option<CChunk> {
        let mut at = chunk.parent;
        for _ in 0..64 {
            let row = self.db.chunk(at?)?;
            if row.kind == ChunkKind::Function {
                return Some(row);
            }
            at = row.parent;
        }
        None
    }
}
