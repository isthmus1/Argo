//! The client cache as [`Msg`]s for the event loop, across connections.
//!
//! Callbacks only mark a table dirty: a burst of them (one transaction, or ingest batches arriving
//! back to back) becomes one rebuild, nothing is rebuilt before the subscription applies, and the
//! structure is rebuilt only when the `c_chunk` rows really changed. The feed outlives each
//! connection, so a reconnect onto unchanged rows reaches the window as a status-only update.

use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use argo_db_client::{CChunk, RustChunk, Structure, rebuild};

use super::send;
use crate::db::Msg;

/// How long a burst of row callbacks may continue before the rebuild that covers it.
const SETTLE: Duration = Duration::from_millis(40);

/// What a connection's callbacks report.
#[derive(Clone, PartialEq)]
pub enum Mark {
    Applied,
    Structure,
    Status,
    /// The connection or its subscription ended, and why.
    Lost(String),
}

/// The two subscribed tables as the client cache holds them.
pub trait Cache {
    fn chunks(&self) -> Vec<CChunk>;
    fn rust(&self) -> Vec<RustChunk>;
}

pub struct Feed {
    tx: Sender<Msg>,
    version: u64,
    /// The `c_chunk` rows `structure` was built from, by id.
    chunks: Option<Vec<CChunk>>,
    structure: Result<Structure, String>,
    /// Whether the window shows a program: the last message sent was `Rows`.
    shown: bool,
}

impl Feed {
    pub fn new(tx: Sender<Msg>) -> Feed {
        Feed {
            tx,
            version: 0,
            chunks: None,
            structure: Err("subscribing".into()),
            shown: false,
        }
    }

    /// False once the window is gone.
    pub fn send(&self, msg: Msg) -> bool {
        send(&self.tx, msg)
    }

    /// Follow one connection until it is lost: why, and whether its subscription applied.
    pub fn follow(&mut self, cache: &impl Cache, marks: &Receiver<Mark>) -> (String, bool) {
        let mut applied = false;
        while let Ok(first) = marks.recv() {
            std::thread::sleep(SETTLE);
            let burst: Vec<Mark> = std::iter::once(first).chain(marks.try_iter()).collect();
            if let Some(why) = burst.iter().find_map(|m| {
                if let Mark::Lost(why) = m {
                    Some(why.clone())
                } else {
                    None
                }
            }) {
                return (why, applied);
            }
            applied |= burst.contains(&Mark::Applied);
            if !applied {
                continue;
            }
            if burst.iter().any(|m| *m != Mark::Status) {
                self.restructure(cache.chunks());
            }
            self.emit(&cache.rust());
        }
        ("connection closed".into(), applied)
    }

    /// The connection ended. A program on screen stays, marked offline, so the last known work
    /// remains visible while reconnecting; an empty canvas says why it is still empty. False once
    /// the window is gone.
    pub fn lost(&self, why: &str) -> bool {
        self.send(Msg::Online(false))
            && (self.shown || self.send(Msg::Waiting(format!("{why} (retrying)"))))
    }

    fn restructure(&mut self, mut chunks: Vec<CChunk>) {
        chunks.sort_by_key(|c| c.id);
        if self.chunks.as_ref() == Some(&chunks) {
            return;
        }
        self.structure = rebuild(&chunks).map_err(|e| e.to_string());
        self.version += 1;
        self.chunks = Some(chunks);
    }

    fn emit(&mut self, rust: &[RustChunk]) {
        let msg = match &self.structure {
            Ok(s) => Msg::Rows {
                structure: self.version,
                snap: Box::new(s.live(rust)),
                origin: s.origin.clone(),
            },
            Err(why) => Msg::Waiting(why.clone()),
        };
        self.shown = matches!(msg, Msg::Rows { .. });
        self.send(msg);
    }
}

#[cfg(test)]
mod tests;
