//! Rows grouped into `ingest_rows` calls the module accepts.

use crate::limits::{MAX_BATCH_BYTES, MAX_BATCH_ROWS};
use crate::{CChunk, Error};

/// A row's size exactly as `ingest_rows` bounds it (the shared `limits::row_bytes`).
pub fn row_bytes(r: &CChunk) -> usize {
    crate::limits::row_bytes([&r.name, &r.path, &r.ingested, &r.source, &r.records])
}

/// Group rows into `ingest_rows` calls of at most [`MAX_BATCH_ROWS`] rows and [`MAX_BATCH_BYTES`]
/// each; the project row comes first. A row too large to send alone is an error, since the
/// module would reject its batch.
pub fn batches(rows: Vec<CChunk>) -> Result<Vec<Vec<CChunk>>, Error> {
    let mut out: Vec<Vec<CChunk>> = vec![];
    let mut bytes = 0;
    for row in rows {
        let size = row_bytes(&row);
        if size > MAX_BATCH_BYTES {
            return Err(Error::RowTooLarge { id: row.id, size });
        }
        let full = out
            .last()
            .is_none_or(|b| b.len() == MAX_BATCH_ROWS || bytes + size > MAX_BATCH_BYTES);
        if full {
            out.push(vec![]);
            bytes = 0;
        }
        bytes += size;
        out.last_mut().expect("pushed above").push(row);
    }
    Ok(out)
}
