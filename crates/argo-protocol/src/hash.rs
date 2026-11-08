//! Canonical content hash of a graph revision's structure (§4 Freeze).
//!
//! Covers schema version, project/parent identity, analysis level, file hashes and the sorted
//! node, pin, edge and layout records. Excludes the revision's own ID and state, every status
//! record, annotations and anything the GUI owns (positions, viewport, selection). S computes it;
//! never accept a worker's claimed digest.

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    Analysis, ContentHash, Edge, FileRef, Graph, Node, Pin, ProjectId, RevisionId, TypeLayout,
};

pub fn sha256(bytes: &[u8]) -> ContentHash {
    let digest = Sha256::digest(bytes);
    ContentHash(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Serialize)]
struct Canonical<'a> {
    schema_version: u32,
    project: ProjectId,
    parent: Option<RevisionId>,
    analysis: Analysis,
    files: Vec<&'a FileRef>,
    nodes: Vec<&'a Node>,
    pins: Vec<&'a Pin>,
    edges: Vec<&'a Edge>,
    layouts: Vec<&'a TypeLayout>,
}

pub fn canonical_hash(graph: &Graph) -> ContentHash {
    let mut files: Vec<_> = graph.files.iter().collect();
    files.sort_by_key(|f| f.id);
    let mut nodes: Vec<_> = graph.nodes.iter().collect();
    nodes.sort_by_key(|n| n.id);
    let mut pins: Vec<_> = graph.pins.iter().collect();
    pins.sort_by(|a, b| (a.node, &a.key).cmp(&(b.node, &b.key)));
    let mut edges: Vec<_> = graph.edges.iter().collect();
    edges.sort_by_key(|e| e.id);
    let mut layouts: Vec<_> = graph.layouts.iter().collect();
    layouts.sort_by_key(|l| l.node);
    let canonical = Canonical {
        schema_version: graph.schema_version,
        project: graph.revision.project,
        parent: graph.revision.parent,
        analysis: graph.revision.analysis,
        files,
        nodes,
        pins,
        edges,
        layouts,
    };
    sha256(&serde_json::to_vec(&canonical).expect("graph records always serialize"))
}
