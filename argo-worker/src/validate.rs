//! The seam for the compile/link/C-test pipeline (ARCHITECTURE.md §5.8 steps 4–5, §5.9), which
//! teammates are building. A [`Validator`] judges one candidate; the retry loop decides what to
//! do with the verdict. [`CompileCheck`] is the first step of it: the candidate must compile.
//! [`Unvalidated`] checks nothing and never claims a pass.

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

/// Compiles each candidate on its own with `rustc` (`argo_c::comp::compiles`). A candidate that
/// doesn't compile is `Failed`, and rustc's errors go back to the model. One that compiles is
/// still `Unvalidated`: it isn't linked into the C program or tested yet.
pub struct CompileCheck;

impl CompileCheck {
    /// Whether `rustc` can be run, checked before claiming anything: without it every candidate
    /// would fail and use up every chunk's attempts.
    pub fn available() -> Result<(), String> {
        match std::process::Command::new("rustc")
            .arg("--version")
            .output()
        {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(format!("rustc --version failed ({})", out.status)),
            Err(e) => Err(format!("cannot run rustc: {e}")),
        }
    }
}

impl Validator for CompileCheck {
    async fn validate(&self, candidate: &Candidate<'_>) -> Verdict {
        let source = candidate.source.to_string();
        // rustc takes a while; off this task, so the claim's heartbeat keeps running.
        match tokio::task::spawn_blocking(move || argo_c::comp::compiles(&source)).await {
            Ok(Ok(())) => {
                Verdict::Unvalidated("compiled on its own with rustc; not linked or tested".into())
            }
            Ok(Err(log)) => Verdict::Failed(log),
            Err(e) => Verdict::Failed(format!("the compile check did not finish: {e}")),
        }
    }
}

#[cfg(test)]
mod compile_tests {
    use argo_db_client::{CChunk, ChunkKind};

    use super::*;

    fn chunk() -> CChunk {
        CChunk {
            id: 1,
            node: Some(1),
            parent: None,
            name: "add".into(),
            kind: ChunkKind::Function,
            translatable: true,
            depth: 0,
            descendants: 0,
            path: "a.c".into(),
            line_start: 1,
            line_end: 1,
            ingested: "a.i".into(),
            byte_lo: 0,
            byte_hi: 0,
            source: "int add(int a, int b) { return a + b; }".into(),
            records: String::new(),
        }
    }

    async fn verdict(source: &str) -> Verdict {
        let chunk = chunk();
        let candidate = Candidate {
            chunk: &chunk,
            source,
            checkout: None,
        };
        CompileCheck.validate(&candidate).await
    }

    #[tokio::test]
    async fn compiling_rust_is_unvalidated_and_broken_rust_fails_with_rustc_errors() {
        assert!(CompileCheck::available().is_ok());
        let good = "#[no_mangle]\npub extern \"C\" fn add(a: i32, b: i32) -> i32 { a + b }";
        assert!(matches!(verdict(good).await, Verdict::Unvalidated(_)));
        let bad = "#[no_mangle]\npub extern \"C\" fn add(a: i32, b: i32) -> i32 { a + c }";
        match verdict(bad).await {
            Verdict::Failed(log) => assert!(log.contains("cannot find value `c`"), "{log}"),
            other => panic!("{other:?}"),
        }
    }
}
