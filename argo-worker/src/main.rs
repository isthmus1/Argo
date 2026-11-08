//! `argo-worker`: translates claimed C chunks to Rust.
//!
//! Connects to SpacetimeDB, subscribes to `c_chunk` and `rust_chunk`, clones the repository at
//! the commit the project row records, then loops: `claim_next`, wait for its own placeholder,
//! prompt nemotron with the chunk's C and surrounding context, run the `Validator` (unvalidated
//! until the teammates' compile/link/test pipeline lands), retry or escalate, then `submit_rust`
//! or `fail_chunk`, heartbeating while it works.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use argo_db_client::MAX_MODEL_BYTES;
use clap::Parser;

use argo_worker::attempt::{Budget, Escalation};
use argo_worker::checkout::Checkout;
use argo_worker::db::Db;
use argo_worker::model::Nemotron;
use argo_worker::validate::Unvalidated;
use argo_worker::work::Worker;
use argo_worker::workdir;

/// Claim C chunks and submit extern "C" Rust for them.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// SpacetimeDB host, e.g. `ws://127.0.0.1:3100`.
    #[arg(long)]
    db: String,
    /// Database name the module is published under.
    #[arg(long, default_value = "argo")]
    module: String,
    /// Where to clone the repository, held exclusively by this process; a clean clone there is
    /// reused. By default, a free slot under `$XDG_CACHE_HOME/argo-worker/<module>/`.
    #[arg(long)]
    workdir: Option<PathBuf>,
    /// Model for every attempt until escalation.
    #[arg(long, env = "OPENAI_MODEL", value_parser = model_name)]
    model: String,
    /// Generations per chunk before giving up with `fail_chunk`.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    max_attempts: u32,
    /// A stronger model to switch to after `--escalate-after` failed attempts.
    #[arg(long, value_parser = model_name)]
    escalation_model: Option<String>,
    /// Failed attempts before escalating; fewer than `--max-attempts`, or it never happens.
    #[arg(long, default_value_t = 2, requires = "escalation_model")]
    escalate_after: u32,
    /// Translate one chunk, then exit.
    #[arg(long)]
    once: bool,
}

/// The module rejects a longer model name in `submit_rust`; refuse it before any work is done.
fn model_name(name: &str) -> Result<String, String> {
    match name.len() {
        1..=MAX_MODEL_BYTES => Ok(name.to_string()),
        n => Err(format!(
            "model names are 1 to {MAX_MODEL_BYTES} bytes, not {n}"
        )),
    }
}

/// A model request that takes longer is an attempt failure, not a stuck claim.
const MODEL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The first pause when the model endpoint is unavailable.
const BACKOFF: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("argo-worker: {why}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), String> {
    // A worker that cannot reach a model must not claim anything.
    Nemotron::configured()?;
    if args.escalation_model.is_some() && args.escalate_after >= args.max_attempts {
        return Err(format!(
            "--escalate-after {} never escalates within --max-attempts {}",
            args.escalate_after, args.max_attempts
        ));
    }
    // Held until exit, an explicit directory as much as a default slot: no other worker may
    // clone into it or move it meanwhile.
    let workdir = match &args.workdir {
        Some(dir) => workdir::hold(dir)?,
        None => workdir::slot(&args.module)?,
    };
    let db = Db::connect(&args.db, &args.module).await?;
    eprintln!("argo-worker: connected as {}", db.me.to_hex());
    let escalation = args.escalation_model.map(|model| Escalation {
        model,
        after: args.escalate_after,
    });
    let budget = Budget {
        max_attempts: args.max_attempts,
        model: args.model,
        escalation,
        backoff: BACKOFF,
    };
    let model = Nemotron {
        timeout: MODEL_TIMEOUT,
    };
    let mut worker = Worker {
        db,
        model,
        validator: Unvalidated,
        budget,
        checkout: Checkout::new(workdir),
    };
    let outcome = worker.run(args.once).await;
    worker.db.close();
    outcome
}
