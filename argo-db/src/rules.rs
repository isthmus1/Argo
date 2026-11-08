//! Claim, lease and fencing rules as plain functions, so `cargo test` checks them natively
//! without a SpacetimeDB host.

use std::cmp::Reverse;

pub use crate::limits::{
    LEASE_MICROS, MAX_BATCH_BYTES, MAX_BATCH_ROWS, MAX_LOG_BYTES, MAX_MODEL_BYTES, MAX_RUST_BYTES,
    ROW_OVERHEAD, row_bytes,
};
use crate::tables::RustStatus;

/// A translatable chunk that may be claimed now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: u64,
    pub descendants: u32,
    pub depth: u32,
}

/// Leaves first: fewest translatable descendants, then deepest, then lowest id.
pub fn pick(candidates: impl IntoIterator<Item = Candidate>) -> Option<Candidate> {
    candidates
        .into_iter()
        .min_by_key(|c| (c.descendants, Reverse(c.depth), c.id))
}

/// Whether a chunk may be claimed: it has no Rust row, or its `Claimed` lease expired.
/// `existing` is the Rust row's status and `updated_at` in microseconds.
pub fn claimable(existing: Option<(RustStatus, i64)>, now: i64) -> bool {
    match existing {
        None => true,
        Some((RustStatus::Claimed, updated)) => now - updated >= LEASE_MICROS,
        Some(_) => false,
    }
}

/// The generation the next claim gets, from the last one handed out by any claim. One counter
/// for the whole database, never reset, so a generation names exactly one claim even after the
/// row is deleted by `requeue` or `ingest_reset`; a per-row counter would restart at 1 there and
/// let a stale write match a replacement claim.
pub fn next_fence(last: u32) -> Result<u32, String> {
    last.checked_add(1)
        .ok_or_else(|| "fencing generations are exhausted".into())
}

/// A write against a claim: it must still be `Claimed`, by this caller, at this generation.
pub fn check_fence(
    status: RustStatus,
    caller_holds: bool,
    current: u32,
    quoted: u32,
) -> Result<(), String> {
    if status != RustStatus::Claimed {
        return Err(format!("chunk is {status:?}, not Claimed"));
    }
    if !caller_holds {
        return Err("chunk is claimed by another worker".into());
    }
    if current != quoted {
        return Err(format!("stale generation {quoted}; current is {current}"));
    }
    Ok(())
}

/// Reject, never truncate, oversize text: the caller decides what to cut.
pub fn check_len(what: &str, text: &str, max: usize) -> Result<(), String> {
    if text.len() > max {
        return Err(format!("{what} is {} bytes; limit {max}", text.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
