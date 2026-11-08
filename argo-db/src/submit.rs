//! Finishing a claim. Every write quotes the chunk and fencing generation it was handed; stale,
//! duplicate and foreign writes are rejected with an error, never silently accepted.

use spacetimedb::{ReducerContext, reducer};

use crate::claim::held;
use crate::owner::owner;
use crate::rules::{MAX_LOG_BYTES, MAX_MODEL_BYTES, MAX_RUST_BYTES, check_len};
use crate::tables::{CChunk, RustChunk, RustStatus, Validation, c_chunk, rust_chunk};

/// Publish generated Rust for a held claim: `Claimed → Done`. A failed validation is not a
/// result; report it with `fail_chunk` (or retry) instead.
#[allow(clippy::too_many_arguments)]
#[reducer]
pub fn submit_rust(
    ctx: &ReducerContext,
    chunk_id: u64,
    generation: u32,
    source: String,
    c_call: String,
    model: String,
    attempts: u32,
    validation: Validation,
    log: String,
) -> Result<(), String> {
    let row = held(ctx, chunk_id, generation)?;
    if validation == Validation::Failed {
        return Err("a failed candidate is not a result; use fail_chunk".into());
    }
    if source.trim().is_empty() {
        return Err("empty Rust source".into());
    }
    check_len("source", &source, MAX_RUST_BYTES)?;
    check_len("c_call", &c_call, MAX_RUST_BYTES)?;
    // The translation beside its C, in the chunk's own row.
    if let Some(chunk) = ctx.db.c_chunk().id().find(chunk_id) {
        ctx.db.c_chunk().id().update(CChunk { rust: source.clone(), c_call, ..chunk });
    }
    check_len("model", &model, MAX_MODEL_BYTES)?;
    check_len("log", &log, MAX_LOG_BYTES)?;
    ctx.db.rust_chunk().chunk().update(RustChunk {
        status: RustStatus::Done,
        updated_at: ctx.timestamp,
        attempts,
        model,
        source,
        validation,
        log,
        ..row
    });
    Ok(())
}

/// Give up on a held claim: `Claimed → Failed`, terminal until an owner requeues it.
#[reducer]
pub fn fail_chunk(
    ctx: &ReducerContext,
    chunk_id: u64,
    generation: u32,
    attempts: u32,
    log: String,
) -> Result<(), String> {
    let row = held(ctx, chunk_id, generation)?;
    check_len("log", &log, MAX_LOG_BYTES)?;
    ctx.db.rust_chunk().chunk().update(RustChunk {
        status: RustStatus::Failed,
        updated_at: ctx.timestamp,
        attempts,
        log,
        ..row
    });
    Ok(())
}

/// Owner only: delete a chunk's Rust row so it can be claimed again. The fencing counter lives
/// in the config row, so the next claim still gets a fresh generation and writes quoting the
/// deleted claim's generation keep failing.
#[reducer]
pub fn requeue(ctx: &ReducerContext, chunk_id: u64) -> Result<(), String> {
    owner(ctx)?;
    if !ctx.db.rust_chunk().chunk().delete(chunk_id) {
        return Err(format!("chunk {chunk_id} has no Rust row"));
    }
    if let Some(chunk) = ctx.db.c_chunk().id().find(chunk_id) {
        ctx.db.c_chunk().id().update(CChunk { rust: String::new(), c_call: String::new(), ..chunk });
    }
    Ok(())
}
