//! Claim selection, the stop condition and idle rejections over hand-made rows.

use std::collections::HashSet;

use argo_db_client::{CChunk, ChunkKind, RustChunk, RustStatus, Validation};
use spacetimedb_sdk::{Identity, Timestamp};

use super::{idle, mine, open, submitted};

const ME: Identity = Identity::from_byte_array([1; 32]);
const OTHER: Identity = Identity::from_byte_array([2; 32]);

fn rust(chunk: u64, worker: Identity, generation: u32, status: RustStatus) -> RustChunk {
    RustChunk {
        chunk,
        status,
        worker,
        generation,
        claimed_at: Timestamp::UNIX_EPOCH,
        updated_at: Timestamp::UNIX_EPOCH,
        attempts: 0,
        model: String::new(),
        source: String::new(),
        validation: Validation::Unvalidated,
        log: String::new(),
    }
}

fn c(id: u64, translatable: bool) -> CChunk {
    CChunk {
        id,
        node: Some(id),
        parent: None,
        name: format!("f{id}"),
        kind: ChunkKind::Function,
        translatable,
        depth: 1,
        descendants: 0,
        path: "f.c".into(),
        line_start: 1,
        line_end: 1,
        ingested: "f.c".into(),
        byte_lo: 0,
        byte_hi: 1,
        source: String::new(),
        records: "{}".into(),
    }
}

#[test]
fn mine_is_the_oldest_own_claim_not_yet_handled() {
    let rows = vec![
        rust(1, ME, 1, RustStatus::Claimed),
        rust(2, ME, 4, RustStatus::Claimed),
        rust(3, OTHER, 9, RustStatus::Claimed),
        rust(4, ME, 7, RustStatus::Done),
    ];
    let none = HashSet::new();
    assert_eq!(mine(rows.clone(), ME, &none).map(|r| r.chunk), Some(1));
    assert_eq!(mine(rows.clone(), OTHER, &none).map(|r| r.chunk), Some(3));
    // Claim 4 was adopted first because claim 1's outcome was lost: 1 is still worked.
    let later = HashSet::from([4]);
    assert_eq!(mine(rows.clone(), ME, &later).map(|r| r.chunk), Some(1));
    let both = HashSet::from([1, 4]);
    assert_eq!(
        mine(rows, ME, &both),
        None,
        "a handled claim is not a new one"
    );
}

#[test]
fn a_submission_is_own_done_row_at_the_claim_generation() {
    assert!(submitted(&rust(1, ME, 3, RustStatus::Done), ME, 3));
    assert!(!submitted(&rust(1, ME, 3, RustStatus::Claimed), ME, 3));
    assert!(!submitted(&rust(1, ME, 3, RustStatus::Failed), ME, 3));
    assert!(
        !submitted(&rust(1, ME, 4, RustStatus::Done), ME, 3),
        "a later claim's"
    );
    assert!(!submitted(&rust(1, OTHER, 3, RustStatus::Done), ME, 3));
}

#[test]
fn open_until_every_translatable_chunk_is_done_or_failed() {
    let chunks = [c(1, true), c(2, true), c(3, false)];
    let done = rust(1, ME, 1, RustStatus::Done);
    assert!(
        open(&chunks, std::slice::from_ref(&done)),
        "chunk 2 is unclaimed"
    );
    let leased = rust(2, OTHER, 2, RustStatus::Claimed);
    assert!(open(&chunks, &[done.clone(), leased]), "a lease may expire");
    let failed = rust(2, OTHER, 2, RustStatus::Failed);
    assert!(
        !open(&chunks, &[done, failed]),
        "structure rows are never work"
    );
    assert!(!open(&[], &[]), "nothing ingested, nothing to do");
}

#[test]
fn only_transient_rejections_are_idle() {
    assert!(idle("no chunk is available"));
    assert!(idle("ingest is not finished"));
    assert!(!idle("fencing generations are exhausted"));
}
