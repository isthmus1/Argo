//! Ingested source text cut into bounded `Source` rows, and joined back.

use std::collections::BTreeMap;
use std::ops::Range;

use argo_protocol::{FileId, SourceFile};

use super::SEGMENT_BYTES;
use crate::Error;
use crate::records::SourceMeta;

/// Every file's segments: the file, the segment's index within it and its byte range.
pub fn segments(files: &[SourceFile]) -> Vec<(&SourceFile, u32, Range<usize>)> {
    files
        .iter()
        .flat_map(|f| {
            split(&f.text)
                .into_iter()
                .zip(0..)
                .map(move |(r, i)| (f, i, r))
        })
        .collect()
}

/// Split text into segments of at most [`SEGMENT_BYTES`], on character boundaries.
fn split(text: &str) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut lo = 0;
    while lo < text.len() {
        let mut hi = (lo + SEGMENT_BYTES).min(text.len());
        while !text.is_char_boundary(hi) {
            hi -= 1;
        }
        out.push(lo..hi);
        lo = hi;
    }
    out
}

pub fn meta(f: &SourceFile, segments: &[(&SourceFile, u32, Range<usize>)]) -> SourceMeta {
    SourceMeta {
        id: f.id,
        path: f.path.clone(),
        sha256: f.sha256.clone(),
        segments: segments.iter().filter(|(s, ..)| s.id == f.id).count() as u32,
    }
}

/// A file's text from its segments, checked against the hash recorded at ingest.
pub fn source(
    m: &SourceMeta,
    segments: &BTreeMap<(FileId, u32), &str>,
) -> Result<SourceFile, Error> {
    let text = (0..m.segments)
        .map(|i| segments.get(&(m.id, i)).copied())
        .collect::<Option<String>>()
        .ok_or_else(|| Error::Corrupt(format!("missing a segment of {}", m.path)))?;
    let file = SourceFile::new(m.id, m.path.clone(), text);
    if file.sha256 != m.sha256 {
        return Err(Error::Corrupt(format!(
            "{} does not match its hash",
            m.path
        )));
    }
    Ok(file)
}
