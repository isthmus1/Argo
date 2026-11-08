//! The seam for the compile/link/C-test pipeline (ARCHITECTURE.md §5.8 steps 4–5, §5.9), which
//! teammates are building. A [`Validator`] judges one candidate; the retry loop decides what to
//! do with the verdict. Until a real one exists, [`Unvalidated`] says so and never claims a pass.

use std::future::Future;
use std::path::Path;

use argo_db_client::CChunk;

/// One generated candidate for a claimed chunk.
pub struct Candidate<'a> {
    pub chunk: &'a CChunk,
    /// Fence-stripped Rust that passed the shape check (`#[no_mangle] pub extern "C"`).
    pub source: &'a str,
    /// The repository at the ingested commit, to build and test against; `None` when the
    /// ingest recorded no commit to clone. Read-only: build in a directory of your own (a copy
    /// or `git worktree`), since any file left here, ignored ones included, makes the worker
    /// refuse the clone at its next claim (`checkout::Checkout::sync`).
    pub checkout: Option<&'a Path>,
}

/// What a validator established. Each carries the log fed back into the next prompt (on
/// failure) or submitted with the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Built, linked into the C program and its tests passed.
    Passed(String),
    /// Nothing was checked: submitted as `Validation::Unvalidated`.
    Unvalidated(String),
    /// Rejected: the log goes back to the model and the attempt counts against the budget.
    Failed(String),
}

pub trait Validator {
    /// Judge a candidate. Blocking work (compilers, test runs) belongs on a blocking thread.
    /// The future need not be `Send`: [`crate::work::Worker`] runs on its caller's task.
    fn validate(&self, candidate: &Candidate<'_>) -> impl Future<Output = Verdict>;
}

/// The honest default until the pipeline lands: every candidate is reported unvalidated.
pub struct Unvalidated;

impl Validator for Unvalidated {
    async fn validate(&self, _: &Candidate<'_>) -> Verdict {
        Verdict::Unvalidated("not compiled, linked or tested: no validator is configured".into())
    }
}
