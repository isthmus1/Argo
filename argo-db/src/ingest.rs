//! Ingest. Reducers never read files: the native `argo-server` parses the
//! repository and pushes bounded batches of C chunk rows here.

use spacetimedb::{ReducerContext, Table, reducer};

use crate::owner::{Config, config, owner};
use crate::rules::{MAX_BATCH_BYTES, MAX_BATCH_ROWS, row_bytes};
use crate::tables::{CChunk, ChunkKind, c_chunk, rust_chunk};

/// Start (or resume) ingesting `locator` at `commit`. The same pair again is a no-op; a different
/// one needs `ingest_reset` first, so a commit is never silently mixed with another.
#[reducer]
pub fn ingest_begin(ctx: &ReducerContext, locator: String, commit: String) -> Result<(), String> {
    let c = owner(ctx)?;
    if locator.is_empty() {
        return Err("empty repository locator".into());
    }
    if c.locator.is_empty() {
        ctx.db.config().key().update(Config {
            locator,
            commit,
            ..c
        });
        return Ok(());
    }
    if (c.locator.as_str(), c.commit.as_str()) == (locator.as_str(), commit.as_str()) {
        return Ok(());
    }
    Err(format!(
        "database holds {}@{}; call ingest_reset first",
        c.locator, c.commit
    ))
}

/// Upsert a bounded batch. After sealing, only byte-identical rows (a replay) are accepted.
#[reducer]
pub fn ingest_rows(ctx: &ReducerContext, rows: Vec<CChunk>) -> Result<(), String> {
    let c = owner(ctx)?;
    if c.locator.is_empty() {
        return Err("call ingest_begin first".into());
    }
    if rows.len() > MAX_BATCH_ROWS {
        return Err(format!("{} rows; limit {MAX_BATCH_ROWS}", rows.len()));
    }
    let bytes = rows.iter().fold(0usize, |n, r| {
        n.saturating_add(row_bytes([
            &r.name,
            &r.path,
            &r.ingested,
            &r.source,
            &r.records,
        ]))
    });
    if bytes > MAX_BATCH_BYTES {
        return Err(format!("batch is {bytes} bytes; limit {MAX_BATCH_BYTES}"));
    }
    let table = ctx.db.c_chunk();
    for row in rows {
        match table.id().find(row.id) {
            // A translation written since doesn't make a replayed row differ.
            Some(old) if old.ingested() == row.ingested() => {}
            _ if c.sealed => return Err(format!("ingest is sealed; row {} differs", row.id)),
            Some(_) => drop(table.id().update(row)),
            None => drop(table.insert(row)),
        }
    }
    Ok(())
}

/// Seal once exactly `total` rows, one of them the project root, are present.
#[reducer]
pub fn ingest_finish(ctx: &ReducerContext, total: u64) -> Result<(), String> {
    let c = owner(ctx)?;
    let chunks = ctx.db.c_chunk();
    let have = chunks.count();
    if have != total {
        return Err(format!("{have} rows present; expected {total}"));
    }
    let projects = chunks.iter().filter(|r| r.kind == ChunkKind::Project);
    if projects.count() != 1 {
        return Err("expected exactly one Project row".into());
    }
    ctx.db.config().key().update(Config { sealed: true, ..c });
    Ok(())
}

/// Drop every chunk and claim so a different commit can be ingested. The owner and the fencing
/// counter survive, so a write quoting a claim from the old commit can never match a new one.
#[reducer]
pub fn ingest_reset(ctx: &ReducerContext) -> Result<(), String> {
    let c = owner(ctx)?;
    let claims: Vec<_> = ctx.db.rust_chunk().iter().map(|r| r.chunk).collect();
    for chunk in claims {
        ctx.db.rust_chunk().chunk().delete(chunk);
    }
    let chunks: Vec<_> = ctx.db.c_chunk().iter().map(|r| r.id).collect();
    for id in chunks {
        ctx.db.c_chunk().id().delete(id);
    }
    ctx.db.config().key().update(Config {
        locator: String::new(),
        commit: String::new(),
        sealed: false,
        ..c
    });
    Ok(())
}
