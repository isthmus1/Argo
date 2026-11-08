//! The retry loop: generate, shape-check, validate. Each failure's log goes into the next
//! prompt; after `escalation.after` failures the escalation model takes over (opt-in, so never a
//! silent switch, §5.8); after `max_attempts` the chunk is given up. An unavailable endpoint is
//! waited out instead, and if it stays down the chunk is left to its lease, never failed.

use argo_db_client::Validation;
use tokio::time::sleep;

use crate::log::{MAX_LOG_BYTES, tail};
use crate::model::{Model, ModelError};
use crate::job::Job;
use crate::validate::{Candidate, Validator, Verdict};
use crate::{fence, shape};

mod budget;

use budget::OUTAGES;
pub use budget::{Budget, Escalation};

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Never `Validation::Failed`: a failing candidate is retried, not submitted.
    Done {
        source: String,
        /// For a piece of a function: the C call that replaces it.
        c: Option<String>,
        model: String,
        attempts: u32,
        validation: Validation,
        log: String,
    },
    Failed {
        attempts: u32,
        log: String,
    },
    /// The endpoint stayed unavailable: nothing is recorded, the claim lapses with its lease.
    Unavailable {
        log: String,
    },
}

/// Translate `chunk`. `prompt` builds the request from the previous failure's log, if any.
pub async fn translate(
    model: &impl Model,
    validator: &impl Validator,
    budget: &Budget,
    job: &Job<'_>,
    prompt: impl Fn(Option<&str>) -> String,
) -> Outcome {
    let mut log = String::new();
    let mut feedback = None;
    let (mut failures, mut outages) = (0, 0);
    while failures < budget.max_attempts {
        let (attempts, name) = (failures + 1, budget.model(failures));
        let request = prompt(feedback.as_deref());
        match once(model, validator, name, &request, job).await {
            Ok((source, c, validation, text)) => {
                log.push_str(&format!("attempt {attempts} ({name}): {text}\n"));
                let (model, log) = (name.to_string(), tail(&log, MAX_LOG_BYTES));
                return Outcome::Done {
                    source,
                    c,
                    model,
                    attempts,
                    validation,
                    log,
                };
            }
            Err(ModelError::Unavailable(why)) => {
                outages += 1;
                log.push_str(&format!("outage {outages} ({name}): {why}\n"));
                if outages > OUTAGES {
                    let log = tail(&log, MAX_LOG_BYTES);
                    return Outcome::Unavailable { log };
                }
                sleep(budget.pause(outages)).await;
            }
            Err(ModelError::Reply(why)) => {
                outages = 0;
                failures += 1;
                log.push_str(&format!("attempt {attempts} ({name}): {why}\n"));
                feedback = Some(why);
            }
        }
    }
    Outcome::Failed {
        attempts: failures,
        log: tail(&log, MAX_LOG_BYTES),
    }
}

/// One attempt: the candidate (and, for a piece, the C that replaces it), what validation
/// established and its log; or why it failed.
async fn once(
    model: &impl Model,
    validator: &impl Validator,
    name: &str,
    prompt: &str,
    job: &Job<'_>,
) -> Result<(String, Option<String>, Validation, String), ModelError> {
    let reply = model.complete(name, prompt).await?;
    let source = fence::strip(&reply);
    let c = match &job.piece {
        None => None,
        Some(_) => Some(fence::c_blocks(&reply).ok_or_else(|| {
            ModelError::Reply("no ```c block: give the C that replaces the TARGET lines".into())
        })?),
    };
    shape::check(job.chunk, job.symbol(), &source).map_err(ModelError::Reply)?;
    let candidate = Candidate {
        chunk: job.chunk,
        source: &source,
        checkout: job.checkout,
        flags: job.flags,
        symbol: job.symbol(),
        c: c.as_deref(),
    };
    match validator.validate(&candidate).await {
        Verdict::Passed(log) => Ok((source, c, Validation::Passed, log)),
        Verdict::Unvalidated(log) => Ok((source, c, Validation::Unvalidated, log)),
        Verdict::Failed(log) => Err(ModelError::Reply(format!("validation failed:\n{log}"))),
    }
}

#[cfg(test)]
mod tests;
