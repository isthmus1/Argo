//! The two public domain tables. The private owner/config row lives in `owner`.
//!
//! The module stores rows and enforces transitions; it never interprets the serialized protocol
//! records a chunk carries. `argo-db-client` owns both conversions, so the server and the GUI
//! agree on what a row means.

use spacetimedb::{Identity, SpacetimeType, Timestamp, table};

/// What a C chunk stands for. Only work units are translatable; the rest exist so a subscriber
/// can rebuild the whole graph from the database alone.
#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkKind {
    /// The one root row: revision metadata, repository locator and commit, file manifest.
    Project,
    /// A bounded segment of an ingested source file's exact text.
    Source,
    Module,
    Function,
    /// A work unit inside a function (a statement region translated on its own).
    Region,
    Type,
    State,
    /// Any other top-level item (opaque declarations and the like).
    Other,
}

#[table(accessor = c_chunk, public)]
#[derive(Clone, Debug, PartialEq)]
pub struct CChunk {
    /// Assigned by the ingesting client, deterministically for one snapshot, so re-ingest of the
    /// same project and commit rewrites identical rows.
    #[primary_key]
    pub id: u64,
    /// The protocol node this chunk is headed by; `None` for source segments.
    pub node: Option<u64>,
    /// The enclosing chunk, following containment.
    pub parent: Option<u64>,
    pub name: String,
    pub kind: ChunkKind,
    pub translatable: bool,
    /// Containment depth in the chunk tree (root = 0); deeper chunks are claimed first.
    pub depth: u32,
    /// Translatable chunks nested inside this one; leaves (0) are claimed first.
    pub descendants: u32,
    /// The original repository file (relative to its root) that `line_start..=line_end` refer
    /// to: where the chunk's first and last bytes came from, per the preprocessor's line markers.
    /// When they can't be traced to one project file, the enclosing module's file, lines 0.
    /// Empty for `Project` and `Source` rows.
    pub path: String,
    /// 1-based inclusive lines in `path`, for reading context from a checkout; 0 when unknown.
    pub line_start: u32,
    pub line_end: u32,
    /// The ingested source file that `byte_lo..byte_hi` and `source` index: the preprocessed
    /// combination of every unit (its text is in the `Source` rows), not a repository file.
    pub ingested: String,
    pub byte_lo: u32,
    pub byte_hi: u32,
    /// Exact text of `ingested` at `byte_lo..byte_hi`: preprocessed C (includes resolved,
    /// macros expanded, comments gone), the code that was parsed and is to be translated.
    pub source: String,
    /// JSON protocol records this chunk owns (see `argo-db-client`).
    pub records: String,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RustStatus {
    /// A placeholder: a worker holds the lease and is generating.
    Claimed,
    Done,
    /// Terminal: not grabbed again until an owner requeues it.
    Failed,
}

/// What the worker's validator established. `Unvalidated` is the honest default until the
/// compile/link/C-test pipeline exists.
#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validation {
    Unvalidated,
    Passed,
    /// Reserved for a validator that keeps a failing candidate for inspection. The reducers
    /// never store it today: `submit_rust` rejects it, and `fail_chunk` marks the row `Failed`
    /// through its status instead.
    Failed,
}

#[table(accessor = rust_chunk, public)]
#[derive(Clone, Debug, PartialEq)]
pub struct RustChunk {
    /// The C chunk's id. One row per chunk is what makes a claim atomic.
    #[primary_key]
    pub chunk: u64,
    pub status: RustStatus,
    pub worker: Identity,
    /// Fencing generation: bumped on every (re)claim; writes must quote the current one.
    pub generation: u32,
    pub claimed_at: Timestamp,
    pub updated_at: Timestamp,
    pub attempts: u32,
    pub model: String,
    /// Empty while `Claimed`.
    pub source: String,
    pub validation: Validation,
    pub log: String,
}
