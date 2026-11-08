//! The worker's SpacetimeDB connection: one fresh identity per process (no saved token, since a
//! worker finds its claim by identity), a subscription to both tables, and reducer calls that
//! await their outcome. What the subscription shows is read in [`cache`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use argo_db_client::DbConnection;
use argo_db_client::bindings::{ReducerEventContext, RemoteReducers};
use spacetimedb_sdk::error::InternalError;
use spacetimedb_sdk::{DbContext, Identity};
use tokio::sync::oneshot;
use tokio::time::timeout;

mod cache;

/// How long a reducer call, the connection or a claim's arrival in the cache may take.
pub const WAIT: Duration = Duration::from_secs(30);

/// The completion callback every generated `*_then` reducer method takes.
pub type Reply =
    Box<dyn FnOnce(&ReducerEventContext, Result<Result<(), String>, InternalError>) + Send>;

/// Why a reducer call did not succeed. The distinction matters: a rejection is final, while an
/// unknown outcome (not sent, timed out, dropped) may hide a call that committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The module returned an error (or failed internally): nothing was committed.
    Rejected(String),
    /// No outcome arrived; the call may or may not have committed.
    Unknown(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Rejected(why) => write!(f, "rejected: {why}"),
            CallError::Unknown(why) => write!(f, "outcome unknown: {why}"),
        }
    }
}

type Connected = Mutex<Option<oneshot::Sender<Result<Identity, String>>>>;

/// Report the connection outcome once, from whichever callback fires first.
fn settle(slot: &Connected, outcome: Result<Identity, String>) {
    if let Some(tx) = slot.lock().unwrap().take() {
        let _ = tx.send(outcome);
    }
}

pub struct Db {
    conn: DbConnection,
    pub me: Identity,
    /// Set by [`Db::close`], so the disconnect it causes is not taken for a lost connection.
    closing: Arc<AtomicBool>,
}

impl Db {
    /// Connect and wait until the subscription to both tables is applied. A later disconnect
    /// not asked for by [`Db::close`] ends the process: any claim it held is reclaimed once its
    /// lease runs out.
    pub async fn connect(uri: &str, module: &str) -> Result<Db, String> {
        let closing = Arc::new(AtomicBool::new(false));
        let expected = closing.clone();
        let (tx, rx) = oneshot::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let (ok, err) = (tx.clone(), tx);
        let conn = DbConnection::builder()
            .with_uri(uri)
            .with_database_name(module)
            .on_connect(move |_, id, _| settle(&ok, Ok(id)))
            .on_connect_error(move |_, e| settle(&err, Err(e.to_string())))
            .on_disconnect(move |_, e| {
                if !expected.load(Ordering::SeqCst) {
                    eprintln!("argo-worker: disconnected from SpacetimeDB: {e:?}");
                    std::process::exit(1);
                }
            })
            .build()
            .map_err(|e| format!("connecting to {uri}: {e}"))?;
        conn.run_threaded();
        let me = timeout(WAIT, rx)
            .await
            .map_err(|_| "no connection")?
            .map_err(|_| "no connection")??;
        let (tx, rx) = oneshot::channel();
        conn.subscription_builder()
            .on_applied(move |_| {
                let _ = tx.send(());
            })
            .subscribe_to_all_tables();
        timeout(WAIT, rx)
            .await
            .map_err(|_| "subscription never applied")?
            .map_err(|_| "subscription failed")?;
        Ok(Db { conn, me, closing })
    }

    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let _ = self.conn.disconnect();
    }

    /// Invoke a reducer through its generated `*_then` method and await its outcome:
    /// `db.call(|r, f| r.claim_next_then(f))`.
    pub async fn call(
        &self,
        invoke: impl FnOnce(&RemoteReducers, Reply) -> spacetimedb_sdk::Result<()>,
    ) -> Result<(), CallError> {
        let (tx, rx) = oneshot::channel();
        let reply: Reply = Box::new(move |_, r| {
            let _ = tx.send(r.map_err(|e| e.to_string()).and_then(|r| r));
        });
        let unknown = |why: String| CallError::Unknown(why);
        invoke(&self.conn.reducers, reply).map_err(|e| unknown(format!("not sent: {e}")))?;
        timeout(WAIT, rx)
            .await
            .map_err(|_| unknown("no reducer outcome".into()))?
            .map_err(|_| unknown("reducer outcome dropped".into()))?
            .map_err(CallError::Rejected)
    }
}
