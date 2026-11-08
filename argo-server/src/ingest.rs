//! Ingest through the owner's connection: begin, send what the database lacks, finish.
//!
//! Row ids are positional, so the same repository and commit always produce the same rows.
//! Rows already present and identical are not sent again: a restart is a no-op once sealed, and
//! resumes an interrupted ingest otherwise. The module rejects a different locator or commit,
//! and differing rows once sealed; `--fresh` is the explicit `ingest_reset` before beginning.

use std::collections::HashMap;

use argo_db_client::bindings::{
    CChunkTableAccess, ingest_begin, ingest_finish, ingest_reset, ingest_rows,
};
use argo_db_client::{CChunk, batches};
use spacetimedb_sdk::{DbContext, SubscriptionHandle, Table};

use crate::conn::{Conn, reply};
use crate::repo::Loaded;

/// Rows in this ingest, and how many of them had to be sent.
pub struct Sent {
    pub total: usize,
    pub sent: usize,
}

pub async fn ingest(conn: &Conn, loaded: &Loaded, fresh: bool) -> Result<Sent, String> {
    let reducers = &conn.db.reducers;
    if fresh {
        conn.call(|f| reducers.ingest_reset_then(f))
            .await
            .map_err(|e| format!("ingest_reset: {e}"))?;
    }
    let present = present(conn).await?;
    let locator = loaded.origin.locator.clone();
    let commit = loaded.origin.commit.clone().unwrap_or_default();
    conn.call(|f| reducers.ingest_begin_then(locator, commit, f))
        .await
        .map_err(|e| format!("ingest_begin: {e} (restart with --fresh to replace it)"))?;
    let todo = missing(&loaded.rows, &present);
    let sent = todo.len();
    for batch in batches(todo).map_err(|e| e.to_string())? {
        conn.call(|f| reducers.ingest_rows_then(batch, f))
            .await
            .map_err(|e| format!("ingest_rows: {e} (restart with --fresh to replace it)"))?;
    }
    let total = loaded.rows.len();
    conn.call(|f| reducers.ingest_finish_then(total as u64, f))
        .await
        .map_err(|e| {
            format!("ingest_finish: {e} (rows of another ingest? restart with --fresh)")
        })?;
    Ok(Sent { total, sent })
}

/// The rows the database doesn't hold exactly, in order.
fn missing(rows: &[CChunk], present: &HashMap<u64, CChunk>) -> Vec<CChunk> {
    rows.iter()
        // A translation written into a row since (`rust`, `c_call`) doesn't make it differ.
        .filter(|r| present.get(&r.id).map(ingested) != Some(ingested(r)))
        .cloned()
        .collect()
}

/// A row as ingested, without the translation `submit_rust` writes into it.
fn ingested(r: &CChunk) -> CChunk {
    CChunk { rust: String::new(), c_call: String::new(), ..r.clone() }
}

/// The `c_chunk` rows the database already holds. Subscribed only for this look, so the
/// batches sent afterwards don't stream back.
async fn present(conn: &Conn) -> Result<HashMap<u64, CChunk>, String> {
    let (applied, rx) = reply();
    let failed = applied.clone();
    let sub = conn
        .db
        .subscription_builder()
        .on_applied(move |_| applied.send(Ok(())))
        .on_error(move |_, e| failed.send(Err(e.to_string())))
        .subscribe("SELECT * FROM c_chunk");
    conn.or_lost(rx)
        .await?
        .map_err(|e| format!("subscribing to c_chunk: {e}"))?;
    let rows = conn.db.db.c_chunk().iter().map(|r| (r.id, r)).collect();
    sub.unsubscribe().map_err(|e| e.to_string())?;
    Ok(rows)
}

/// One line saying what is now in the database.
pub fn report(loaded: &Loaded, sent: &Sent) {
    let o = &loaded.origin;
    let commit = o.commit.as_deref().unwrap_or("no commit");
    let units = o.units.iter().filter(|u| u.skipped.is_none()).count();
    let work = loaded.rows.iter().filter(|r| r.translatable).count();
    eprintln!(
        "argo-server: {}@{commit}: {} rows ({work} translatable; {units}/{} units), {} sent",
        o.locator,
        sent.total,
        o.units.len(),
        sent.sent
    );
}

#[cfg(test)]
mod tests {
    use argo_db_client::ChunkKind;

    use super::*;

    fn row(id: u64, source: &str) -> CChunk {
        CChunk {
            id,
            node: Some(id),
            parent: None,
            name: format!("f{id}"),
            kind: ChunkKind::Function,
            translatable: true,
            depth: 0,
            descendants: 0,
            path: "a.c".into(),
            line_start: 1,
            line_end: 1,
            ingested: "p.i".into(),
            byte_lo: 0,
            byte_hi: source.len() as u32,
            source: source.into(),
            records: String::new(),
            rust: String::new(),
            c_call: String::new(),
        }
    }

    #[test]
    fn only_absent_or_differing_rows_are_sent() {
        let rows = [row(1, "a"), row(2, "b"), row(3, "c")];
        assert_eq!(missing(&rows, &HashMap::new()), rows);
        let all = rows.iter().map(|r| (r.id, r.clone())).collect();
        assert!(missing(&rows, &all).is_empty(), "a restart sends nothing");
        let partial = HashMap::from([(1, row(1, "a")), (2, row(2, "changed"))]);
        assert_eq!(missing(&rows, &partial), [row(2, "b"), row(3, "c")]);
    }
}
