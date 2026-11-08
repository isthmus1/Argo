//! Claiming work. A claim is the insertion of a `Claimed` placeholder Rust row in the same
//! transaction that picks the chunk, so two workers can never be handed the same chunk.

use spacetimedb::{ReducerContext, Table, reducer};

use crate::limits::{NONE_FREE, NOT_SEALED};
use crate::owner::{Config, config};
use crate::rules::{Candidate, check_fence, claimable, next_fence, pick};
use crate::tables::{RustChunk, RustStatus, Validation, c_chunk, rust_chunk};

fn micros(t: spacetimedb::Timestamp) -> i64 {
    t.to_micros_since_unix_epoch()
}

/// Claim the next free translatable chunk, leaves first. The worker learns which one from its
/// subscription: a `Claimed` row whose `worker` is its own identity. Worker processes
/// must therefore not share one identity (no shared saved credential); the unique generation
/// still keeps each one's writes fenced from the others' claims.
#[reducer]
pub fn claim_next(ctx: &ReducerContext) -> Result<(), String> {
    let cfg = config(ctx).filter(|c| c.sealed).ok_or(NOT_SEALED)?;
    let now = micros(ctx.timestamp);
    let (chunks, rust) = (ctx.db.c_chunk(), ctx.db.rust_chunk());
    let free = chunks.iter().filter(|c| c.translatable).filter(|c| {
        let existing = rust.chunk().find(c.id);
        claimable(existing.map(|r| (r.status, micros(r.updated_at))), now)
    });
    let chosen = pick(free.map(|c| Candidate {
        id: c.id,
        descendants: c.descendants,
        depth: c.depth,
    }))
    .ok_or(NONE_FREE)?;
    let generation = next_fence(cfg.fence)?;
    ctx.db.config().key().update(Config {
        fence: generation,
        ..cfg
    });
    let row = RustChunk {
        chunk: chosen.id,
        status: RustStatus::Claimed,
        worker: ctx.sender(),
        generation,
        claimed_at: ctx.timestamp,
        updated_at: ctx.timestamp,
        attempts: 0,
        model: String::new(),
        source: String::new(),
        validation: Validation::Unvalidated,
        log: String::new(),
    };
    if rust.chunk().find(chosen.id).is_some() {
        rust.chunk().update(row);
    } else {
        rust.insert(row);
    }
    Ok(())
}

/// The claim a write refers to, if the caller still holds it at `generation`.
pub fn held(ctx: &ReducerContext, chunk: u64, generation: u32) -> Result<RustChunk, String> {
    let row = ctx
        .db
        .rust_chunk()
        .chunk()
        .find(chunk)
        .ok_or_else(|| format!("chunk {chunk} is not claimed"))?;
    check_fence(
        row.status,
        row.worker == ctx.sender(),
        row.generation,
        generation,
    )?;
    Ok(row)
}

/// Renew the lease on a held claim.
#[reducer]
pub fn heartbeat(ctx: &ReducerContext, chunk_id: u64, generation: u32) -> Result<(), String> {
    let row = held(ctx, chunk_id, generation)?;
    ctx.db.rust_chunk().chunk().update(RustChunk {
        updated_at: ctx.timestamp,
        ..row
    });
    Ok(())
}
