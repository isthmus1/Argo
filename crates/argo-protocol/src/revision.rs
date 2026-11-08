use serde::{Deserialize, Serialize};

use crate::{ContentHash, Edge, FileRef, Node, Pin, ProjectId, RevisionId, TypeLayout};

/// One immutable-once-frozen graph revision (§4 `GraphRevision`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRevision {
    pub id: RevisionId,
    pub project: ProjectId,
    pub parent: Option<RevisionId>,
    pub analysis: Analysis,
    pub state: RevisionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum RevisionState {
    /// W is still publishing batches. The GUI renders it as pending.
    Draft,
    /// S checked structure and computed this canonical hash. Structural validity is not
    /// behavioral correctness.
    Frozen { hash: ContentHash },
}

/// How much the producer actually derived. The GUI and S must not read more into a graph than this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Analysis {
    /// Containment, templates, exec order and lexically present exits. Data pins may be missing.
    SyntaxOnly,
    /// Names and types resolved: signature pins and type/call relations are trustworthy.
    Resolved,
    /// Live-in/live-out and effects derived: every region's data pins are complete.
    Interfaces,
}

/// The structure of one revision: everything [`canonical_hash`](crate::canonical_hash) covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub schema_version: u32,
    pub revision: GraphRevision,
    pub files: Vec<FileRef>,
    pub nodes: Vec<Node>,
    pub pins: Vec<Pin>,
    pub edges: Vec<Edge>,
    pub layouts: Vec<TypeLayout>,
}

/// One structural record of a `publish_graph_batch` call (§4: at most 128 per batch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "record")]
pub enum GraphRecord {
    File(FileRef),
    Node(Node),
    Pin(Pin),
    Edge(Edge),
    Layout(TypeLayout),
}

pub const MAX_BATCH: usize = 128;

impl Graph {
    pub fn empty(revision: GraphRevision) -> Self {
        Self {
            schema_version: crate::SCHEMA_VERSION,
            revision,
            files: Vec::new(),
            nodes: Vec::new(),
            pins: Vec::new(),
            edges: Vec::new(),
            layouts: Vec::new(),
        }
    }

    pub fn insert(&mut self, record: GraphRecord) {
        match record {
            GraphRecord::File(f) => self.files.push(f),
            GraphRecord::Node(n) => self.nodes.push(n),
            GraphRecord::Pin(p) => self.pins.push(p),
            GraphRecord::Edge(e) => self.edges.push(e),
            GraphRecord::Layout(l) => self.layouts.push(l),
        }
    }

    /// Split into publishable batches, files and nodes first so pins and edges never dangle
    /// within an applied prefix.
    pub fn batches(&self) -> Vec<Vec<GraphRecord>> {
        let records: Vec<GraphRecord> = self
            .files
            .iter()
            .cloned()
            .map(GraphRecord::File)
            .chain(self.nodes.iter().cloned().map(GraphRecord::Node))
            .chain(self.pins.iter().cloned().map(GraphRecord::Pin))
            .chain(self.layouts.iter().cloned().map(GraphRecord::Layout))
            .chain(self.edges.iter().cloned().map(GraphRecord::Edge))
            .collect();
        records.chunks(MAX_BATCH).map(<[_]>::to_vec).collect()
    }

    /// Freeze: validate and stamp the canonical hash. Fails without changing the revision.
    pub fn freeze(&mut self) -> Result<ContentHash, Vec<crate::Violation>> {
        let violations = crate::validate(self);
        if !violations.is_empty() {
            return Err(violations);
        }
        let hash = crate::canonical_hash(self);
        self.revision.state = RevisionState::Frozen { hash: hash.clone() };
        Ok(hash)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}
