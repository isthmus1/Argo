//! A snapshot's structure as `c_chunk` rows, and the bounded batches `ingest_rows` accepts.

use argo_protocol::{FileId, Snapshot, SourceRange};

use locate::{Texts, blank};
use own::own;
use source::{meta, segments};

use crate::partition::partition;
use crate::records::{Origin, Project, Segment};
use crate::{CChunk, ChunkKind, Error};

/// Source text is stored in segments of at most this many bytes.
pub const SEGMENT_BYTES: usize = 256 << 10;

/// Traces a byte of an ingested source file to the original repository file and 1-based line it
/// came from; `None` where that is unknown. For `argo::project::open` that is its `places`
/// (each unit's source is its preprocessed items laid end to end, whose offsets say nothing
/// about the repository's files); for a source that is itself an original file, its own lines.
pub type Places<'a> = dyn Fn(FileId, u32) -> Option<(String, u32)> + 'a;

/// The project row's id. Ids are positional, so the same snapshot always yields the same rows.
pub const ROOT: u64 = 1;

/// Every row of one ingest: the project row, source segments, then one row per chunk head.
pub fn to_rows(
    snapshot: &Snapshot,
    origin: &Origin,
    places: &Places,
) -> Result<Vec<CChunk>, Error> {
    let graph = &snapshot.graph;
    let segments = segments(&snapshot.sources);
    let first = ROOT + 1 + segments.len() as u64;
    let parts = partition(graph, ROOT, first);
    let mut owned = own(graph, &parts);
    let project = Project {
        schema_version: graph.schema_version,
        revision: graph.revision.clone(),
        origin: origin.clone(),
        files: graph.files.clone(),
        sources: snapshot
            .sources
            .iter()
            .map(|f| meta(f, &segments))
            .collect(),
        chunks: first + parts.heads.len() as u64 - ROOT,
        owned: owned.remove(&ROOT).unwrap_or_default(),
    };
    let texts = Texts::new(snapshot);
    let program = graph
        .nodes
        .iter()
        .find(|n| parts.owner(n.id) == ROOT && n.parent.is_none());
    let mut rows = vec![CChunk {
        node: program.map(|n| n.id.0),
        name: program.map_or_else(|| origin.locator.clone(), |n| n.name.clone()),
        descendants: parts.heads.iter().filter(|h| h.node.work_unit).count() as u32,
        records: serde_json::to_string(&project)?,
        ..blank(ROOT, ChunkKind::Project, None)
    }];
    for ((f, index, range), id) in segments.iter().zip(ROOT + 1..) {
        let at = SourceRange {
            file: f.id,
            lo: range.start as u32,
            hi: range.end as u32,
        };
        let row = CChunk {
            name: f.path.clone(),
            records: serde_json::to_string(&Segment {
                file: f.id,
                index: *index,
            })?,
            ..blank(id, ChunkKind::Source, Some(ROOT))
        };
        rows.push(texts.slice(row, at));
    }
    for h in &parts.heads {
        let row = CChunk {
            node: Some(h.node.id.0),
            name: h.node.name.clone(),
            translatable: h.node.work_unit,
            depth: h.depth,
            descendants: h.descendants,
            path: h.path.clone(),
            records: serde_json::to_string(&owned.remove(&h.id).unwrap_or_default())?,
            ..blank(h.id, h.kind, Some(h.parent))
        };
        rows.push(match h.node.provenance.as_ref().map(|p| p.range) {
            Some(r) => texts.origin(texts.slice(row, r), r, places),
            None => row,
        });
    }
    Ok(rows)
}

mod batch;
mod locate;
mod own;
pub(crate) mod source;
pub use batch::{batches, row_bytes};
