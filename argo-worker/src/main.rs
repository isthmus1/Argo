//! `argo-worker`: translates claimed C chunks to Rust.
//!
//! Connects to SpacetimeDB, subscribes to `c_chunk` and `rust_chunk`, clones the repository at
//! the commit the project row records, then loops: `claim_next`, wait for its own placeholder,
//! prompt nemotron with the chunk's C and surrounding context, compile the candidate with
//! `rustc` (`CompileCheck`; rustc's errors go back into the next prompt), retry or escalate, then
//! `submit_rust` or `fail_chunk`, heartbeating while it works. Linking into the C program and
//! testing come later, so a result that compiles is still recorded `Unvalidated`.
//!
//! `--jobs N` runs N such workers side by side, each claiming one chunk at a time; together they
//! drain the open chunks like a pool. Each has its own connection (so its own identity and
//! claims) and its own clone (`--workdir DIR` becomes `DIR/0`, `DIR/1`, ...).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use argo_db_client::MAX_MODEL_BYTES;
use clap::Parser;

use argo_worker::attempt::{Budget, Escalation};
use argo_worker::checkout::Checkout;
use argo_worker::db::Db;
use argo_worker::model::Nemotron;
use argo_worker::validate::CompileCheck;
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
    /// Workers to run in this process, each translating one chunk (one `c_chunk` row) at a time
    /// with its own database connection and clone. Run more processes to add workers later.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    jobs: u32,
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
    // A worker that cannot reach a model, or compile what it returns, must not claim anything.
    Nemotron::configured()?;
    CompileCheck::available()?;
    if args.escalation_model.is_some() && args.escalate_after >= args.max_attempts {
        return Err(format!(
            "--escalate-after {} never escalates within --max-attempts {}",
            args.escalate_after, args.max_attempts
        ));
    }
    let escalation = args.escalation_model.clone().map(|model| Escalation {
        model,
        after: args.escalate_after,
    });
    let budget = Budget {
        max_attempts: args.max_attempts,
        model: args.model.clone(),
        escalation,
        backoff: BACKOFF,
    };
    // Each worker's clone directory and connection first, here on the multi-threaded runtime:
    // the SDK's connect blocks in place, which it can't do inside the local task set below.
    let mut workers = Vec::new();
    for job in 0..args.jobs {
        // Held until the worker ends, an explicit directory as much as a default slot: no other
        // worker may clone into it or move it meanwhile.
        let workdir = match &args.workdir {
            Some(dir) if args.jobs > 1 => workdir::hold(&dir.join(job.to_string()))?,
            Some(dir) => workdir::hold(dir)?,
            None => workdir::slot(&args.module)?,
        };
        let db = Db::connect(&args.db, &args.module).await?;
        eprintln!(
            "argo-worker: worker {job} connected as {} (clone in {})",
            db.me.to_hex(),
            workdir.dir.display()
        );
        workers.push(Worker {
            db,
            model: Nemotron {
                timeout: MODEL_TIMEOUT,
            },
            validator: CompileCheck,
            budget: budget.clone(),
            checkout: Checkout::new(workdir),
        });
    }
    // Workers' futures aren't `Send` (see `Worker`), so they share one local task set; the slow
    // parts (model requests, rustc on a blocking thread) wait without holding it. An error ends
    // only its own worker; the others keep going.
    let local = tokio::task::LocalSet::new();
    let once = args.once;
    let jobs: Vec<_> = workers
        .into_iter()
        .map(|mut worker| {
            local.spawn_local(async move {
                let outcome = worker.run(once).await;
                worker.db.close();
                outcome
            })
        })
        .collect();
    let outcomes = local
        .run_until(async {
            let mut outcomes = Vec::new();
            for job in jobs {
                outcomes.push(job.await.unwrap_or_else(|e| Err(format!("panicked: {e}"))));
            }
            outcomes
        })
        .await;
    let failed: Vec<String> = outcomes
        .into_iter()
        .enumerate()
        .filter_map(|(job, r)| r.err().map(|e| format!("worker {job}: {e}")))
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("\n"))
    }
}
