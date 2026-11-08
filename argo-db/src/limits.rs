//! Limits both sides must agree on. `argo-db-client` compiles this same file (`#[path]`), so the
//! client batches and displays leases by exactly the numbers the module enforces; it stays free
//! of dependencies for that reason.

/// A `Claimed` placeholder whose `updated_at` is older than this may be reclaimed.
pub const LEASE_MICROS: i64 = 10 * 60 * 1_000_000;
/// At most this many rows per `ingest_rows` call (ARCHITECTURE.md §4 graph batch bound).
pub const MAX_BATCH_ROWS: usize = 128;
/// Bytes per `ingest_rows` call, counted by [`row_bytes`]; a single larger row is rejected
/// rather than split. Probed against the installed 2.0.2 host: a batch of about this size is
/// accepted.
pub const MAX_BATCH_BYTES: usize = 16 << 20;
/// A conservative bound on a `c_chunk` row's encoding beyond its five strings: the fixed-size
/// columns (52 bytes in BSATN) plus each string's length prefix, rounded up.
pub const ROW_OVERHEAD: usize = 128;

/// Worker logs are capped (§4: 128 KiB report excerpt); oversize logs are rejected, so workers
/// cut their own.
pub const MAX_LOG_BYTES: usize = 128 << 10;
/// Submitted Rust source (§4: 1 MiB per source file).
pub const MAX_RUST_BYTES: usize = 1 << 20;
/// A recorded model request (its new messages) or reply. The proxy takes requests up to 8 MiB;
/// a delta past this is refused, and the proxy reports it rather than stopping.
pub const MAX_EXCHANGE_BYTES: usize = 4 << 20;
/// A model name: every subscriber receives it, so it stays small.
pub const MAX_MODEL_BYTES: usize = 256;
/// `claim_next`'s rejections that clear by themselves, which a worker waits out rather than
/// stopping for: the ingest is still arriving, or every open chunk is leased.
pub const NOT_SEALED: &str = "ingest is not finished";
pub const NONE_FREE: &str = "no chunk is available";

/// A `c_chunk` row's size for the batch bound, from its name, path, ingested file name, source
/// and records.
pub fn row_bytes(texts: [&str; 5]) -> usize {
    texts
        .iter()
        .fold(ROW_OVERHEAD, |n, t| n.saturating_add(t.len()))
}
