//! The server's SDK connection and the waits it needs: for the connection itself, for a
//! subscription and for a reducer's outcome.
//!
//! The SDK drops neither pending reducer callbacks nor subscription callbacks when the
//! connection is lost, so every wait also races [`Conn::lost`], which `on_disconnect` and the
//! end of the driving task both trigger.

use std::sync::{Arc, Mutex};

use argo_db_client::DbConnection;
use argo_db_client::bindings::ReducerEventContext;
use spacetimedb_sdk::__codegen::InternalError;
use spacetimedb_sdk::{DbContext, Identity};
use tokio::sync::{oneshot, watch};

/// A reducer's completion callback, as the generated `*_then` methods take it.
pub type Done =
    Box<dyn FnOnce(&ReducerEventContext, Result<Result<(), String>, InternalError>) + Send>;

pub struct Conn {
    pub db: Arc<DbConnection>,
    /// Why the connection ended, once it has.
    lost: watch::Receiver<Option<String>>,
}

/// Connect anonymously (a fresh identity each run; the CLI grants it ownership) and drive the
/// connection on a task.
pub async fn connect(uri: &str, db: &str) -> Result<(Conn, Identity), String> {
    let (reply, rx) = reply();
    let on_error = reply.clone();
    let (lose, lost) = watch::channel(None);
    let lose = Arc::new(lose);
    let on_disconnect = lose.clone();
    let db = DbConnection::builder()
        .with_uri(uri)
        .with_database_name(db)
        .on_connect(move |_, id, _| reply.send(Ok(id)))
        .on_connect_error(move |_, e| on_error.send(Err(e.to_string())))
        .on_disconnect(move |_, e| {
            let why = e.map_or_else(
                || "connection lost".into(),
                |e| format!("connection lost: {e}"),
            );
            on_disconnect.send_replace(Some(why));
        })
        .build()
        .map_err(|e| format!("cannot connect to {uri}: {e}"))?;
    let db = Arc::new(db);
    let driver = db.clone();
    tokio::spawn(async move {
        let why = match driver.run_async().await {
            Ok(()) => "connection closed".to_string(),
            Err(e) => format!("connection lost: {e}"),
        };
        // `on_disconnect`'s reason, when it gave one, is the better one.
        lose.send_if_modified(|lost| {
            let unset = lost.is_none();
            if unset {
                *lost = Some(why);
            }
            unset
        });
    });
    let conn = Conn { db, lost };
    let id = conn
        .or_lost(rx)
        .await?
        .map_err(|e| format!("cannot connect to {uri}: {e}"))?;
    Ok((conn, id))
}

impl Conn {
    /// Wait for `rx`, failing if the connection ends first.
    pub async fn or_lost<T>(&self, rx: oneshot::Receiver<T>) -> Result<T, String> {
        or_lost(rx, self.lost.clone()).await
    }

    /// Invoke a reducer through its `*_then` method and wait for its outcome.
    pub async fn call(
        &self,
        invoke: impl FnOnce(Done) -> spacetimedb_sdk::Result<()>,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        invoke(Box::new(move |_, r| {
            let _ = tx.send(r.map_err(|e| e.to_string()).and_then(|r| r));
        }))
        .map_err(|e| e.to_string())?;
        self.or_lost(rx).await?
    }

    /// Close the connection; the driving task then ends.
    pub fn close(&self) {
        if self.db.is_active() {
            let _ = self.db.disconnect();
        }
    }
}

async fn or_lost<T>(
    rx: oneshot::Receiver<T>,
    mut lost: watch::Receiver<Option<String>>,
) -> Result<T, String> {
    // An outcome that arrived just before the connection ended still counts.
    tokio::select! {
        biased;
        r = rx => r.map_err(|_| "the callback was dropped".to_string()),
        why = lost.wait_for(Option::is_some) => Err(match why {
            Ok(why) => why.clone().unwrap_or_default(),
            Err(_) => "the connection's watcher ended".into(),
        }),
    }
}

/// One outcome shared by a success and a failure callback, each `FnOnce` and mutually
/// exclusive in the SDK; the first to fire wins.
#[derive(Clone)]
pub struct Reply<T>(Arc<Mutex<Option<oneshot::Sender<T>>>>);

pub fn reply<T>() -> (Reply<T>, oneshot::Receiver<T>) {
    let (tx, rx) = oneshot::channel();
    (Reply(Arc::new(Mutex::new(Some(tx)))), rx)
}

impl<T> Reply<T> {
    pub fn send(&self, value: T) {
        if let Some(tx) = self.0.lock().expect("never poisoned").take() {
            let _ = tx.send(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wait whose callback never fires (the SDK on disconnect) ends when the connection does.
    #[tokio::test]
    async fn waits_end_when_the_connection_does() {
        let (lose, lost) = watch::channel(None);
        let (_never, rx) = oneshot::channel::<()>();
        let wait = tokio::spawn(or_lost(rx, lost.clone()));
        lose.send_replace(Some("host gone".into()));
        assert_eq!(wait.await.unwrap(), Err("host gone".into()));
        let (tx, rx) = oneshot::channel();
        tx.send(1).unwrap();
        assert_eq!(
            or_lost(rx, lost).await,
            Ok(1),
            "an answer already there still counts"
        );
    }
}
