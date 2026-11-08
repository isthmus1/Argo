//! One Rust row as the protocol updates that place it on its node.

use argo_protocol::work::{Abi, Attempt, Implementation, Job, JobState, Stage};
use argo_protocol::{
    AttemptId, Candidate, Claim, ClaimPhase, Failure, ImplementationId, JobId, Language, NodeId,
    NodeStatus, ReportId, Update, Verdict, WorkerId, hash::sha256,
};

use super::report::report;
use crate::limits::LEASE_MICROS;
use crate::{RustChunk, RustStatus, Validation};

pub fn updates(r: &RustChunk, node: NodeId, worker: WorkerId) -> Vec<Update> {
    let (job, attempt) = (
        JobId(r.chunk),
        AttemptId((r.chunk << 32) | u64::from(r.generation)),
    );
    let implementation = ImplementationId(r.chunk);
    let report_id = ReportId(r.chunk);
    let mut status = NodeStatus::new(node, Language::C);
    let state = match (r.status, r.validation) {
        (RustStatus::Claimed, _) => {
            status.claim = Some(Claim {
                worker,
                job,
                attempt,
                stage: Stage::Translate,
                phase: ClaimPhase::Running,
            });
            JobState::Running
        }
        (RustStatus::Done, Validation::Passed) => {
            status.accepted = Language::Rust;
            status.implementation = Some(implementation);
            status.evidence = vec![report_id];
            JobState::Succeeded
        }
        (RustStatus::Done, _) => {
            let verdict = Verdict::Unvalidated;
            status.candidate = Some(Candidate {
                implementation,
                language: Language::Rust,
                verdict,
            });
            JobState::Succeeded
        }
        (RustStatus::Failed, _) => {
            // The row carries no finer failure class: the worker exhausted its attempts.
            let failure = Failure::Unsupported;
            let verdict = Verdict::Failed {
                report: report_id,
                failure,
            };
            status.candidate = Some(Candidate {
                implementation,
                language: Language::Rust,
                verdict,
            });
            JobState::Failed
        }
    };
    // Only a live claim has a lease; a finished row reports when it finished.
    let lease = if r.status == RustStatus::Claimed {
        LEASE_MICROS
    } else {
        0
    };
    let micros = r.updated_at.to_micros_since_unix_epoch() + lease;
    let mut out = vec![
        Update::Job(Job {
            id: job,
            stage: Stage::Translate,
            logical_key: format!("translate:{}", r.chunk),
            targets: vec![node],
            prerequisites: vec![],
            state,
        }),
        Update::Attempt(Attempt {
            id: attempt,
            job,
            worker,
            fence: u64::from(r.generation),
            lease_expires_ms: (micros / 1000).max(0) as u64,
            state,
        }),
        Update::Status(status),
    ];
    if r.status != RustStatus::Claimed {
        let preview = (!r.source.is_empty()).then(|| r.source.clone());
        out.push(Update::Implementation(Implementation {
            id: implementation,
            node,
            language: Language::Rust,
            abi: Abi::C,
            source_hash: sha256(r.source.as_bytes()),
            preview,
        }));
    }
    out.extend(report(r, implementation, report_id));
    out
}
