use super::*;

fn c(id: u64, descendants: u32, depth: u32) -> Candidate {
    Candidate {
        id,
        descendants,
        depth,
    }
}

#[test]
fn leaves_then_deepest_then_lowest_id() {
    let parent = c(1, 2, 2);
    let shallow_leaf = c(2, 0, 2);
    let deep_leaf = c(3, 0, 3);
    let twin = c(4, 0, 3);
    assert_eq!(
        pick([parent, shallow_leaf, twin, deep_leaf]),
        Some(deep_leaf)
    );
    assert_eq!(pick([parent, shallow_leaf]), Some(shallow_leaf));
    assert_eq!(pick([]), None);
}

#[test]
fn unclaimed_chunks_and_expired_leases_are_claimable() {
    let now = LEASE_MICROS * 3;
    assert!(claimable(None, now));
    assert!(claimable(
        Some((RustStatus::Claimed, now - LEASE_MICROS)),
        now
    ));
}

#[test]
fn live_claims_and_finished_rows_are_taken() {
    let now = LEASE_MICROS * 3;
    let fresh = now - LEASE_MICROS + 1;
    assert!(!claimable(Some((RustStatus::Claimed, fresh)), now));
    assert!(!claimable(Some((RustStatus::Done, 0)), now));
    assert!(!claimable(Some((RustStatus::Failed, 0)), now));
}

#[test]
fn fences_only_move_forward_and_never_wrap() {
    assert_eq!(next_fence(0), Ok(1));
    assert_eq!(next_fence(41), Ok(42));
    assert!(next_fence(u32::MAX).is_err());
}

#[test]
fn fence_rejects_foreign_stale_and_finished_writes() {
    assert_eq!(check_fence(RustStatus::Claimed, true, 2, 2), Ok(()));
    assert!(check_fence(RustStatus::Claimed, false, 2, 2).is_err());
    assert!(check_fence(RustStatus::Claimed, true, 3, 2).is_err());
    assert!(check_fence(RustStatus::Done, true, 2, 2).is_err());
    assert!(check_fence(RustStatus::Failed, true, 2, 2).is_err());
}

#[test]
fn oversize_text_is_rejected_not_truncated() {
    assert_eq!(check_len("log", "abc", 3), Ok(()));
    assert!(check_len("log", "abcd", 3).is_err());
}

#[test]
fn batch_size_counts_every_string_plus_overhead() {
    assert_eq!(row_bytes(["", "", "", "", ""]), ROW_OVERHEAD);
    assert_eq!(row_bytes(["ab", "c", "hi", "def", "g"]), ROW_OVERHEAD + 9);
    let name = "n".repeat(MAX_BATCH_BYTES);
    assert!(row_bytes([&name, "", "", "", ""]) > MAX_BATCH_BYTES);
}
