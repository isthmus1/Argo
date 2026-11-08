//! SIGINT and SIGTERM as a stop request.
//!
//! Installed before the host is spawned: the host runs in its own process group, so if a signal
//! killed `argo-server` by default the host would be orphaned.

use tokio::signal::unix::{Signal, SignalKind, signal};

pub struct Interrupts {
    int: Signal,
    term: Signal,
}

impl Interrupts {
    pub fn install() -> std::io::Result<Interrupts> {
        Ok(Interrupts {
            int: signal(SignalKind::interrupt())?,
            term: signal(SignalKind::terminate())?,
        })
    }

    /// Resolves on the next SIGINT or SIGTERM, naming it.
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.int.recv() => "SIGINT",
            _ = self.term.recv() => "SIGTERM",
        }
    }
}
