//! Rows back into a snapshot: the GUI's only source of graph structure and source text.

use std::collections::{BTreeMap, HashMap};

use argo_protocol::{FileId, Graph, NodeId, Snapshot};

use crate::records::{At, Origin, Owned, Project, Segment};
use crate::rows::source::source;
use crate::{CChunk, ChunkKind, Error};

/// The ingested program as rows describe it, before any Rust row is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// Every node as originally ingested (C, or no language for structure).
    pub snapshot: Snapshot,
    pub origin: Origin,
    /// Each chunk's head node, to place `rust_chunk` rows on the graph.
    pub nodes: HashMap<u64, NodeId>,
}

/// Rebuild from a complete set of `c_chunk` rows (any order). Fails with
/// [`Error::Incomplete`] while an ingest is still arriving.
pub fn rebuild<'a>(chunks: impl IntoIterator<Item = &'a CChunk>) -> Result<Structure, Error> {
    let chunks: Vec<&CChunk> = chunks.into_iter().collect();
    let mut roots = chunks.iter().filter(|c| c.kind == ChunkKind::Project);
    let root = roots.next().ok_or(Error::NoProject)?;
    if roots.next().is_some() {
        return Err(Error::Corrupt("two project rows".into()));
    }
    let project = complete(root, chunks.len() as u64)?;
    let mut owned = project.owned;
    let mut segments: BTreeMap<(FileId, u32), &str> = BTreeMap::new();
    let mut nodes = HashMap::new();
    for c in &chunks {
        if let Some(n) = c.node {
            nodes.insert(c.id, NodeId(n));
        }
        match c.kind {
            ChunkKind::Project => {}
            ChunkKind::Source => {
                let s: Segment = serde_json::from_str(&c.records)?;
                if segments.insert((s.file, s.index), &c.source).is_some() {
                    return Err(Error::Corrupt(format!("duplicate segment {s:?}")));
                }
            }
            _ => {
                let o: Owned = serde_json::from_str(&c.records)?;
                owned.nodes.extend(o.nodes);
                owned.pins.extend(o.pins);
                owned.edges.extend(o.edges);
                owned.layouts.extend(o.layouts);
            }
        }
    }
    let graph = Graph {
        schema_version: project.schema_version,
        revision: project.revision,
        files: project.files,
        nodes: ordered(owned.nodes, "node")?,
        pins: ordered(owned.pins, "pin")?,
        edges: ordered(owned.edges, "edge")?,
        layouts: ordered(owned.layouts, "layout")?,
    };
    let sources = project
        .sources
        .iter()
        .map(|m| source(m, &segments))
        .collect::<Result<_, _>>()?;
    Ok(Structure {
        snapshot: Snapshot::new(graph, sources),
        origin: project.origin,
        nodes,
    })
}

/// Where the ingest came from, once all `rows` of it have arrived, from the project row (id
/// [`ROOT`](crate::ROOT)) alone: a claim loop's per-claim check, without rebuilding the graph.
pub fn origin(root: Option<&CChunk>, rows: u64) -> Result<Origin, Error> {
    let root = root.ok_or(Error::NoProject)?;
    if root.kind != ChunkKind::Project {
        return Err(Error::Corrupt(format!(
            "row {} is not the project row",
            root.id
        )));
    }
    Ok(complete(root, rows)?.origin)
}

/// The project row's record, if the ingest it describes has exactly `have` rows.
fn complete(root: &CChunk, have: u64) -> Result<Project, Error> {
    let project: Project = serde_json::from_str(&root.records)?;
    let want = project.chunks;
    if have < want {
        return Err(Error::Incomplete { have, want });
    }
    if have > want {
        return Err(Error::Corrupt(format!(
            "{have} rows; the project has {want}"
        )));
    }
    Ok(project)
}

/// Records back in their original order; every position must appear exactly once.
fn ordered<T>(mut records: Vec<At<T>>, what: &str) -> Result<Vec<T>, Error> {
    records.sort_by_key(|(i, _)| *i);
    if (0..).zip(&records).any(|(want, (i, _))| *i != want) {
        return Err(Error::Corrupt(format!("{what} records are not contiguous")));
    }
    Ok(records.into_iter().map(|(_, r)| r).collect())
}
