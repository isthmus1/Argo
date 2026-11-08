//! The SpacetimeDB side of the feed: connections on their own thread, one subscription to both
//! tables per connection, and a [`Feed`] turning the client cache into [`Msg`]s.
//!
//! Row callbacks are registered before subscribing and the first rebuild reads the cache once the
//! subscription is applied, so no change can fall between an initial read and the live stream.
//! A lost connection (host restart, network drop, host not up yet) is retried with bounded
//! backoff; each new connection resubscribes and rebuilds from its own applied cache.

mod feed;

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use argo_db_client::bindings::{CChunkTableAccess, RustChunkTableAccess};
use argo_db_client::{CChunk, DbConnection, RustChunk};
use spacetimedb_sdk::{DbContext, Table, TableWithPrimaryKey};

use super::Msg;
use crate::project::WAKER;
use feed::{Cache, Feed, Mark};

/// Retry delays double from the first to the last, and restart once a subscription applies.
const FIRST_RETRY: Duration = Duration::from_millis(250);
const LAST_RETRY: Duration = Duration::from_secs(8);

pub fn start(uri: String, module: String) -> Receiver<Msg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || run(&uri, &module, Feed::new(tx)));
    rx
}

/// Send and wake the event loop; false once the window is gone.
fn send(tx: &Sender<Msg>, msg: Msg) -> bool {
    let open = tx.send(msg).is_ok();
    if let Some(w) = WAKER.get() {
        w();
    }
    open
}

/// Connect, follow, and reconnect for as long as the window listens.
fn run(uri: &str, module: &str, mut feed: Feed) {
    let mut retry = FIRST_RETRY;
    loop {
        let (why, applied) = session(uri, module, &mut feed);
        if applied {
            retry = FIRST_RETRY;
        }
        if !feed.lost(&why) {
            return;
        }
        std::thread::sleep(retry);
        retry = (retry * 2).min(LAST_RETRY);
    }
}

/// One connection, followed until it is lost: why, and whether its subscription applied.
fn session(uri: &str, module: &str, feed: &mut Feed) -> (String, bool) {
    let (dirty, marks) = channel();
    let lost = dirty.clone();
    let conn = DbConnection::builder()
        .with_uri(uri)
        .with_database_name(module)
        .on_disconnect(move |_, e| {
            let _ = lost.send(Mark::Lost(
                e.map_or_else(|| "disconnected".into(), |e| format!("disconnected: {e}")),
            ));
        })
        .build();
    let conn = match conn {
        Ok(c) => c,
        Err(e) => return (format!("cannot connect to {uri}: {e}"), false),
    };
    feed.send(Msg::Online(true));
    watch(conn.db.c_chunk(), &dirty, Mark::Structure);
    watch(conn.db.rust_chunk(), &dirty, Mark::Status);
    let (applied, failed) = (dirty.clone(), dirty.clone());
    conn.subscription_builder()
        .on_applied(move |_| {
            let _ = applied.send(Mark::Applied);
        })
        .on_error(move |_, e| {
            let _ = failed.send(Mark::Lost(format!("subscription failed: {e}")));
        })
        .subscribe(["SELECT * FROM c_chunk", "SELECT * FROM rust_chunk"]);
    // The SDK's message thread panics on anything but a clean disconnect, skipping
    // `on_disconnect`; its end is the signal that always arrives.
    let thread = conn.run_threaded();
    std::thread::spawn(move || {
        let why = if thread.join().is_ok() {
            "disconnected"
        } else {
            "connection failed"
        };
        let _ = dirty.send(Mark::Lost(why.into()));
    });
    let ended = feed.follow(&conn, &marks);
    // Still open after a failed subscription: close it so its thread ends too.
    let _ = conn.disconnect();
    ended
}

/// Mark `d` dirty on every insert, delete and update of `table`.
fn watch<T: TableWithPrimaryKey>(table: T, dirty: &Sender<Mark>, d: Mark) {
    let (tx, mark) = (dirty.clone(), d.clone());
    table.on_insert(move |_, _| {
        let _ = tx.send(mark.clone());
    });
    let (tx, mark) = (dirty.clone(), d.clone());
    table.on_delete(move |_, _| {
        let _ = tx.send(mark.clone());
    });
    let tx = dirty.clone();
    table.on_update(move |_, _, _| {
        let _ = tx.send(d.clone());
    });
}

impl Cache for DbConnection {
    fn chunks(&self) -> Vec<CChunk> {
        self.db.c_chunk().iter().collect()
    }

    fn rust(&self) -> Vec<RustChunk> {
        self.db.rust_chunk().iter().collect()
    }
}
