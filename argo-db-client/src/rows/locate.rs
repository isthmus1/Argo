//! A row's two locations: its exact text in the ingested source, and where that came from in the
//! repository.

use std::collections::HashMap;

use argo_protocol::{FileId, Snapshot, SourceFile, SourceRange};

use super::Places;
use crate::{CChunk, ChunkKind};

pub fn blank(id: u64, kind: ChunkKind, parent: Option<u64>) -> CChunk {
    CChunk {
        id,
        node: None,
        parent,
        name: String::new(),
        kind,
        translatable: false,
        depth: u32::from(parent.is_some()),
        descendants: 0,
        path: String::new(),
        line_start: 0,
        line_end: 0,
        ingested: String::new(),
        byte_lo: 0,
        byte_hi: 0,
        source: String::new(),
        records: String::new(),
    }
}

/// The ingested source files by id.
pub struct Texts<'s>(HashMap<FileId, &'s SourceFile>);

impl<'s> Texts<'s> {
    pub fn new(snapshot: &'s Snapshot) -> Self {
        Texts(snapshot.sources.iter().map(|f| (f.id, f)).collect())
    }

    /// The ingested file, bytes and exact text of `at`. Ranges outside every ingested file leave
    /// them empty rather than guessing.
    pub fn slice(&self, mut row: CChunk, at: SourceRange) -> CChunk {
        let Some(file) = self.0.get(&at.file) else {
            return row;
        };
        let Some(text) = file.slice(at) else {
            return row;
        };
        row.ingested = file.path.clone();
        row.byte_lo = at.lo;
        row.byte_hi = at.hi;
        row.source = text.to_string();
        row
    }

    /// The original file and lines of `at`, when its first and last bytes trace to one file;
    /// otherwise the row keeps its module path with lines 0, never a guessed position.
    pub fn origin(&self, mut row: CChunk, at: SourceRange, places: &Places) -> CChunk {
        let last = at.hi.saturating_sub(1).max(at.lo);
        if let (Some((file, start)), Some((end_file, end))) =
            (places(at.file, at.lo), places(at.file, last))
            && file == end_file
            && start <= end
        {
            row.path = file;
            row.line_start = start;
            row.line_end = end;
        }
        row
    }
}
