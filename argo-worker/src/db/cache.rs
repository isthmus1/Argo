//! Reads of the subscription cache: rows, the ingest's origin, and waits for a claim or a
//! submission to show up.

use std::collections::HashSet;
use std::time::Duration;

use argo_db_client::bindings::{CChunkTableAccess, RustChunkTableAccess};
use argo_db_client::{CChunk, Error, Origin, ROOT, RustChunk, origin};
use spacetimedb_sdk::Table;
use tokio::time::{Instant, sleep};

use super::Db;
use crate::select;

const POLL: Duration = Duration::from_millis(50);

impl Db {
    pub fn chunks(&self) -> Vec<CChunk> {
        self.conn.db.c_chunk().iter().collect()
    }

    pub fn chunk(&self, id: u64) -> Option<CChunk> {
        self.conn.db.c_chunk().id().find(&id)
    }

    pub fn rusts(&self) -> Vec<RustChunk> {
        self.conn.db.rust_chunk().iter().collect()
    }

    /// Where the complete ingest in the cache came from; `None` while there is none (nothing
    /// ingested, or rows still arriving). Reads the project row alone, not the whole graph.
    pub fn ingested(&self) -> Result<Option<Origin>, String> {
        let table = self.conn.db.c_chunk();
        match origin(table.id().find(&ROOT).as_ref(), table.count()) {
            Ok(origin) => Ok(Some(origin)),
            Err(Error::NoProject | Error::Incomplete { .. }) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Whether the cache shows chunk `id` `Done` by this identity at `generation` within
    /// `within`: a `submit_rust` that committed although its outcome was lost.
    pub async fn submitted(&self, id: u64, generation: u32, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        loop {
            let row = self.conn.db.rust_chunk().chunk().find(&id);
            if row.is_some_and(|r| select::submitted(&r, self.me, generation)) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            sleep(POLL).await;
        }
    }

    /// This identity's oldest `Claimed` row not yet `handled`, if the cache has one within
    /// `within`.
    pub async fn claimed(&self, handled: &HashSet<u32>, within: Duration) -> Option<RustChunk> {
        let deadline = Instant::now() + within;
        loop {
            let found = select::mine(self.conn.db.rust_chunk().iter(), self.me, handled);
            if found.is_some() || Instant::now() > deadline {
                return found;
            }
            sleep(POLL).await;
        }
    }
}
