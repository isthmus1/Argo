//! The JSON a chunk row's `records` column carries. The module stores it opaquely; only this
//! crate writes and reads it.

use argo_protocol::{ContentHash, Edge, FileId, FileRef, GraphRevision, Node, Pin, TypeLayout};
use serde::{Deserialize, Serialize};

/// Where the ingested code came from, so a worker can clone exactly it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// Git URL or local path, as given to the server.
    pub locator: String,
    /// The resolved commit (`git rev-parse HEAD`); `None` for a directory that is not a git
    /// checkout, which a worker cannot reproduce exactly.
    pub commit: Option<String>,
    /// Every candidate translation unit and why it was left out, if it was.
    pub units: Vec<Unit>,
    /// Preprocessor flags used, Makefile ones first.
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub path: String,
    pub skipped: Option<String>,
}

/// A record and its position in the original graph vector.
pub type At<T> = (u32, T);

/// The structural records one chunk owns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Owned {
    pub nodes: Vec<At<Node>>,
    pub pins: Vec<At<Pin>>,
    pub edges: Vec<At<Edge>>,
    pub layouts: Vec<At<TypeLayout>>,
}

/// One ingested source file, whose text lives in `Source` rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMeta {
    pub id: FileId,
    pub path: String,
    pub sha256: ContentHash,
    pub segments: u32,
}

/// The project row: everything that isn't owned by a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub revision: GraphRevision,
    pub origin: Origin,
    pub files: Vec<FileRef>,
    pub sources: Vec<SourceMeta>,
    /// Total rows of this ingest, so a reader knows when it has them all.
    pub chunks: u64,
    pub owned: Owned,
}

/// A `Source` row: segment `index` of file `file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub file: FileId,
    pub index: u32,
}
