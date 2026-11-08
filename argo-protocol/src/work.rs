//! Jobs, attempts, implementations, reports and annotations: the records node status points at.
//! Command semantics (claims, fencing, compare-and-swap acceptance) are S's reducers, not these.

use serde::{Deserialize, Serialize};

use crate::{
    AttemptId, ContentHash, Failure, ImplementationId, JobId, Language, NodeId, ReportId, WorkerId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Import,
    Explain,
    Translate,
    EdgeCleanup,
    Collapse,
    Modernize,
    Export,
}

/// `queued → running → succeeded | failed | abandoned` (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub stage: Stage,
    /// Canonical logical-task key: same key, same task (§4).
    pub logical_key: String,
    pub targets: Vec<NodeId>,
    pub prerequisites: Vec<JobId>,
    pub state: JobState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: AttemptId,
    pub job: JobId,
    pub worker: WorkerId,
    /// Monotonic fencing generation; stale fences can't write.
    pub fence: u64,
    /// Server time, milliseconds since the Unix epoch.
    pub lease_expires_ms: u64,
    pub state: JobState,
}

/// Which boundary an implementation is exposed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Abi {
    /// `extern "C"` with the declared or outlined signature and exit tag.
    C,
    /// Native Rust after edge cleanup.
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub id: ImplementationId,
    pub node: NodeId,
    pub language: Language,
    pub abi: Abi,
    pub source_hash: ContentHash,
    /// Bounded inline preview (§4: 128 KiB); full source is fetched by hash.
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Focused cases on every candidate.
    Quick,
    /// The pinned whole-program workload at checkpoints.
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Outcome {
    Passed,
    Failed { failure: Failure },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counterexample {
    pub input: String,
    pub expected: String,
    pub actual: String,
}

/// Evidence bound to one exact candidate (§4). Hashes identify what ran; they don't prove it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub id: ReportId,
    pub implementation: ImplementationId,
    /// The full selected manifest the candidate was built into.
    pub manifest_hash: ContentHash,
    pub tier: Tier,
    pub outcome: Outcome,
    pub cases_passed: u32,
    pub cases_total: u32,
    pub counterexample: Option<Counterexample>,
    /// Bounded excerpt (§4: 128 KiB).
    pub log_excerpt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Summary,
    Assumption,
    /// An open question the explanation couldn't settle.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "author")]
pub enum Author {
    Model { model: String, job: Option<JobId> },
    Human,
    Tool { tool: String },
}

/// An attributed explanation. Never executable truth (§4, §5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub node: NodeId,
    pub kind: AnnotationKind,
    pub text: String,
    pub author: Author,
}
