//! The validation report a finished Rust row carries, if any.

use argo_protocol::work::{Outcome, Tier};
use argo_protocol::{ContentHash, Failure, ImplementationId, ReportId, Update, ValidationReport};

use crate::{RustChunk, RustStatus, Validation};

/// The database records no build manifest yet, so reports name none (all zeros).
const NO_MANIFEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A report for a failure (the worker's log) or a passed validation; none for a live claim or
/// an unvalidated candidate, which nothing has judged.
pub fn report(r: &RustChunk, implementation: ImplementationId, id: ReportId) -> Option<Update> {
    let outcome = match (r.status, r.validation) {
        (RustStatus::Failed, _) => Outcome::Failed {
            failure: Failure::Unsupported,
        },
        (RustStatus::Done, Validation::Passed) => Outcome::Passed,
        _ => return None,
    };
    Some(Update::Report(ValidationReport {
        id,
        implementation,
        manifest_hash: ContentHash::new(NO_MANIFEST),
        tier: Tier::Quick,
        outcome,
        cases_passed: 0,
        cases_total: 0,
        counterexample: None,
        log_excerpt: r.log.clone(),
    }))
}
