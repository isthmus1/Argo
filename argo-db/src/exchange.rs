//! The recorded run: every model request `argo-server`'s proxy forwarded, with the reply, so a
//! run can be inspected afterwards and replayed (`argo-server --replay`), plus the owner's
//! reducer that writes a recorded Rust row into a replay database.
//!
//! A request is stored as a delta of its conversation (one claim's agent): `prefix` messages are
//! the first `prefix` of the previous exchange's full message list with the same `worker`,
//! `chunk` and `generation`, and `request` is the rest of the body with only the messages after
//! them. `prefix` 0 means `request` is the whole body. Without the delta every turn would store
//! the whole conversation again, and the table lives in the host's memory.

use spacetimedb::{ReducerContext, Table, Timestamp, reducer, table};

use crate::owner::{Config, config, owner};
use crate::rules::{MAX_EXCHANGE_BYTES, MAX_LOG_BYTES, MAX_MODEL_BYTES, MAX_RUST_BYTES, check_len};
use crate::tables::{CChunk, RustChunk, RustStatus, c_chunk, rust_chunk};

/// Public, so a replay can read it from another connection. Workers and the GUI subscribe to
/// `c_chunk` and `rust_chunk` only: this table is large.
#[table(accessor = model_exchange, public)]
#[derive(Clone, Debug, PartialEq)]
pub struct ModelExchange {
    /// Assigned on insert, in recording order.
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    /// The proxy's name for the worker (`w3`), as in `usage.log`.
    pub worker: String,
    /// The claim the request was made for; `None` for a worker's startup check.
    pub chunk: Option<u64>,
    pub generation: Option<u32>,
    /// When the proxy received the request and when the reply (or failure) came back.
    pub sent_at: Timestamp,
    pub answered_at: Timestamp,
    /// The provider's HTTP status; 0 when it couldn't be reached.
    pub status: u16,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// USD, as OpenRouter reported it.
    pub cost: f64,
    /// Messages carried over from the conversation's previous exchange (see the module docs).
    pub prefix: u32,
    /// The body as sent upstream (the server's model and provider pin), minus the `prefix`
    /// messages.
    pub request: String,
    /// The provider's reply body, or why there was none.
    pub response: String,
}

/// Owner only: append one exchange (its `id` is assigned here).
#[reducer]
pub fn record_exchange(ctx: &ReducerContext, exchange: ModelExchange) -> Result<(), String> {
    owner(ctx)?;
    check_len("request", &exchange.request, MAX_EXCHANGE_BYTES)?;
    check_len("response", &exchange.response, MAX_EXCHANGE_BYTES)?;
    ctx.db
        .model_exchange()
        .insert(ModelExchange { id: 0, ..exchange });
    Ok(())
}

/// Owner only, for `argo-server --replay`: write a recorded `rust_chunk` row as it was at one
/// point of the recording (claimed, then finished), with the C row's translation (`c_call`).
/// The row gets a fresh fencing generation, and the replay's own clock for its times, so leases
/// read as they did live. Nothing checks it against a claim: the owner is replaying history.
#[reducer]
pub fn replay_rust(ctx: &ReducerContext, row: RustChunk, c_call: String) -> Result<(), String> {
    let cfg = owner(ctx)?;
    if !cfg.sealed {
        return Err("ingest is not finished".into());
    }
    check_len("source", &row.source, MAX_RUST_BYTES)?;
    check_len("c_call", &c_call, MAX_RUST_BYTES)?;
    check_len("model", &row.model, MAX_MODEL_BYTES)?;
    check_len("log", &row.log, MAX_LOG_BYTES)?;
    let chunk = ctx
        .db
        .c_chunk()
        .id()
        .find(row.chunk)
        .ok_or_else(|| format!("no chunk {}", row.chunk))?;
    let rust = ctx.db.rust_chunk();
    let existing = rust.chunk().find(row.chunk);
    let claimed_at = match (&existing, row.status) {
        (Some(old), RustStatus::Done | RustStatus::Failed) => old.claimed_at,
        _ => ctx.timestamp,
    };
    let generation = crate::rules::next_fence(cfg.fence)?;
    ctx.db.config().key().update(Config {
        fence: generation,
        ..cfg
    });
    let done = row.status == RustStatus::Done;
    ctx.db.c_chunk().id().update(CChunk {
        rust: if done {
            row.source.clone()
        } else {
            String::new()
        },
        c_call: if done { c_call } else { String::new() },
        ..chunk
    });
    let row = RustChunk {
        generation,
        claimed_at,
        updated_at: ctx.timestamp,
        ..row
    };
    if existing.is_some() {
        rust.chunk().update(row);
    } else {
        rust.insert(row);
    }
    Ok(())
}
