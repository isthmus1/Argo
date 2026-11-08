//! Storing the model proxy's exchanges in `model_exchange`, through the owner's connection, in
//! the order they were answered. A row the database refuses is reported and skipped: recording
//! is evidence, and never stops the proxy serving workers.

use argo_db_client::ModelExchange;
use argo_db_client::bindings::record_exchange;
use spacetimedb_sdk::Timestamp;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::conn::Conn;
use crate::proxy::Exchange;

/// Record every exchange `rx` yields, for as long as the proxy runs.
pub async fn record(conn: &Conn, mut rx: UnboundedReceiver<Exchange>) -> Result<(), String> {
    let mut refused = 0u64;
    while let Some(exchange) = rx.recv().await {
        let row = row(exchange);
        let reducers = &conn.db.reducers;
        if let Err(why) = conn.call(|f| reducers.record_exchange_then(row, f)).await {
            refused += 1;
            // Every exchange after a lost connection fails the same way; say so now and then.
            if refused <= 3 || refused.is_power_of_two() {
                eprintln!("argo-server: model exchange not recorded ({refused} so far): {why}");
            }
        }
    }
    std::future::pending().await
}

fn row(x: Exchange) -> ModelExchange {
    let tokens = x.tokens.unwrap_or_default();
    ModelExchange {
        id: 0,
        worker: x.worker,
        chunk: x.claim.chunk,
        generation: x.claim.generation,
        sent_at: Timestamp::from(x.sent_at),
        answered_at: Timestamp::from(x.answered_at),
        status: x.status,
        prompt_tokens: tokens.prompt,
        completion_tokens: tokens.completion,
        cost: tokens.cost,
        prefix: x.prefix,
        request: x.request,
        response: x.response,
    }
}
