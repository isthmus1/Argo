//! Decisions the claim loop makes over subscription rows, kept free of the connection so they
//! are tested directly: which row is this worker's claim or submission, whether work remains,
//! and which `claim_next` rejections mean "nothing to do yet".

use std::collections::HashSet;

use argo_db_client::{CChunk, NONE_FREE, NOT_SEALED, RustChunk, RustStatus};
use spacetimedb_sdk::Identity;

/// `me`'s oldest `Claimed` row whose generation isn't in `handled`. Each process has its own
/// identity, so every such row is a claim it made and hasn't worked yet; one whose `claim_next`
/// outcome was lost may surface after a later claim, and is still worked before its lease lapses.
pub fn mine(
    rows: impl IntoIterator<Item = RustChunk>,
    me: Identity,
    handled: &HashSet<u32>,
) -> Option<RustChunk> {
    rows.into_iter()
        .filter(|r| r.worker == me && r.status == RustStatus::Claimed)
        .filter(|r| !handled.contains(&r.generation))
        .min_by_key(|r| r.generation)
}

/// Whether `row` is `me`'s submission for the claim at `generation`.
pub fn submitted(row: &RustChunk, me: Identity, generation: u32) -> bool {
    row.worker == me && row.generation == generation && row.status == RustStatus::Done
}

/// Whether some translatable chunk has no result yet: unclaimed, or claimed (a lease that may
/// still expire and be reclaimed).
pub fn open(chunks: &[CChunk], rust: &[RustChunk]) -> bool {
    let finished: HashSet<u64> = rust
        .iter()
        .filter(|r| r.status != RustStatus::Claimed)
        .map(|r| r.chunk)
        .collect();
    chunks
        .iter()
        .any(|c| c.translatable && !finished.contains(&c.id))
}

/// `claim_next`'s rejections that clear by themselves: the ingest is still arriving, or every
/// open chunk is leased. Anything else is a fault worth stopping for.
pub fn idle(rejection: &str) -> bool {
    [NOT_SEALED, NONE_FREE].contains(&rejection)
}

#[cfg(test)]
mod tests;
