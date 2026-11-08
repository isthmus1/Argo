//! `rust_chunk` rows as protocol status: what the GUI draws on top of the structure.
//!
//! - `Claimed` → a running claim by that worker.
//! - `Done` + `Unvalidated` → an unvalidated Rust candidate; the node stays C.
//! - `Done` + `Passed` → accepted Rust, backed by the validator's report.
//! - `Failed` → a rejected candidate whose report carries the worker's log.

mod report;
mod updates;

use argo_protocol::{Snapshot, Update, Worker, WorkerId};
use spacetimedb_sdk::Identity;

use crate::{RustChunk, RustStatus, Structure};
use updates::updates;

impl Structure {
    /// The live snapshot: the structure plus every Rust row's status. Workers are numbered by
    /// identity order, so numbering is stable while the set of workers is.
    pub fn live<'a>(&self, rust: impl IntoIterator<Item = &'a RustChunk>) -> Snapshot {
        let rows: Vec<&RustChunk> = rust
            .into_iter()
            .filter(|r| self.nodes.contains_key(&r.chunk))
            .collect();
        let mut identities: Vec<Identity> = rows.iter().map(|r| r.worker).collect();
        identities.sort();
        identities.dedup();
        let worker = |id: &Identity| {
            WorkerId(identities.binary_search(id).expect("collected above") as u64 + 1)
        };
        let mut snap = self.snapshot.clone();
        for id in &identities {
            let mine = || rows.iter().filter(move |r| r.worker == *id);
            snap.apply(Update::Worker(Worker {
                id: worker(id),
                label: id.to_hex().to_string()[..8].to_string(),
                model: mine()
                    .max_by_key(|r| r.updated_at)
                    .map_or_else(String::new, |r| r.model.clone()),
                online: mine().any(|r| r.status == RustStatus::Claimed),
            }));
        }
        for r in rows {
            for u in updates(r, self.nodes[&r.chunk], worker(&r.worker)) {
                snap.apply(u);
            }
        }
        snap
    }
}
