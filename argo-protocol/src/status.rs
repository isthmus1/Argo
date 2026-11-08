//! Live migration status. Language and validation are independent dimensions (§4), and nothing
//! here may look accepted without the reports that back it.

use serde::{Deserialize, Serialize};

use crate::{AttemptId, EdgeId, ImplementationId, JobId, NodeId, ReportId, Stage, WorkerId};

/// The implementation language of a node's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Structural nodes (interfaces, dispatch, types before mirroring) have no implementation.
    None,
    C,
    Rust,
}

/// A node's migration state within the accepted manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node: NodeId,
    /// Language of the accepted implementation the manifest currently selects.
    pub accepted: Language,
    pub implementation: Option<ImplementationId>,
    /// Reports that admitted the accepted implementation. Empty for the original C baseline.
    pub evidence: Vec<ReportId>,
    /// The latest unaccepted candidate, if any.
    pub candidate: Option<Candidate>,
    /// A worker's claim on a job targeting this node.
    pub claim: Option<Claim>,
    /// A merged group whose collapse was accepted (§5.10). Displayed as one node until opened.
    pub collapsed: bool,
}

impl NodeStatus {
    pub fn new(node: NodeId, accepted: Language) -> Self {
        Self {
            node,
            accepted,
            implementation: None,
            evidence: Vec::new(),
            candidate: None,
            claim: None,
            collapsed: false,
        }
    }

    /// Accepted Rust backed by at least one report.
    pub fn is_accepted_rust(&self) -> bool {
        self.accepted == Language::Rust && !self.evidence.is_empty()
    }

    pub fn is_rejected(&self) -> bool {
        matches!(
            &self.candidate,
            Some(Candidate {
                verdict: Verdict::Failed { .. },
                ..
            })
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub implementation: ImplementationId,
    pub language: Language,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict")]
pub enum Verdict {
    /// Submitted; no report yet.
    Unvalidated,
    Running,
    /// Reports passed; awaiting compare-and-swap acceptance.
    Passed {
        report: ReportId,
    },
    Failed {
        report: ReportId,
        failure: Failure,
    },
}

/// Distinct failure states with inspectable evidence (§8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Build,
    /// A counterexample: outputs differ from the baseline.
    Mismatch,
    Unsupported,
    Timeout,
    ModelUnavailable,
    /// The base manifest moved; the candidate must be rebased and retested.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub worker: WorkerId,
    pub job: JobId,
    pub attempt: AttemptId,
    pub stage: Stage,
    pub phase: ClaimPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimPhase {
    /// Assigned to the worker but not started.
    Queued,
    Running,
}

/// Whether an edge still crosses a C ABI boundary; edge cleanup (§5.10) clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeStatus {
    pub edge: EdgeId,
    pub crosses_c_abi: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worker {
    pub id: WorkerId,
    pub label: String,
    /// The model it runs, as recorded; capability claims need measurements, not this string.
    pub model: String,
    pub online: bool,
}
