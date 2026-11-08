//! `rust_chunk` rows become the node status the GUI draws.

use argo_db_client::{
    CChunk, LEASE_MICROS, Origin, RustChunk, RustStatus, Structure, Validation, rebuild, to_rows,
};
use argo_protocol::{ClaimPhase, Language, NodeId, Verdict, fixture};
use spacetimedb_sdk::{Identity, Timestamp};

fn structure() -> (Structure, Vec<CChunk>) {
    let origin = Origin {
        locator: "/src/compute".into(),
        commit: None,
        units: vec![],
        flags: vec![],
    };
    let rows = to_rows(&fixture::snapshot(), &origin, &|_, _| None).unwrap();
    (rebuild(&rows).unwrap(), rows)
}

fn rust(
    rows: &[CChunk],
    name: &str,
    worker: u8,
    status: RustStatus,
    validation: Validation,
) -> RustChunk {
    RustChunk {
        chunk: rows
            .iter()
            .find(|r| r.name == name && r.translatable)
            .unwrap()
            .id,
        status,
        worker: Identity::from_byte_array([worker; 32]),
        generation: 2,
        claimed_at: Timestamp::from_micros_since_unix_epoch(1_000),
        updated_at: Timestamp::from_micros_since_unix_epoch(2_000),
        attempts: 1,
        model: format!("model-{worker}"),
        source: if status == RustStatus::Claimed {
            String::new()
        } else {
            format!("// {name}")
        },
        validation,
        log: "log".into(),
    }
}

#[test]
fn rust_rows_map_onto_node_status() {
    let (s, rows) = structure();
    let live = s.live(&[
        rust(&rows, "normalize", 9, RustStatus::Done, Validation::Passed),
        rust(&rows, "step", 3, RustStatus::Done, Validation::Unvalidated),
        rust(
            &rows,
            "compute",
            3,
            RustStatus::Claimed,
            Validation::Unvalidated,
        ),
    ]);
    let node = |name: &str| {
        NodeId(
            rows.iter()
                .find(|r| r.name == name && r.translatable)
                .unwrap()
                .node
                .unwrap(),
        )
    };
    let status = |name| live.status(node(name)).unwrap();

    let normalize = status("normalize");
    assert!(normalize.is_accepted_rust());
    assert_eq!(
        live.report(normalize.evidence[0]).unwrap().log_excerpt,
        "log"
    );

    let step = status("step");
    assert_eq!(step.accepted, Language::C);
    assert!(matches!(
        step.candidate.as_ref().unwrap().verdict,
        Verdict::Unvalidated
    ));
    assert_eq!(
        live.implementations
            .iter()
            .find(|i| i.node == node("step"))
            .unwrap()
            .preview
            .as_deref(),
        Some("// step")
    );

    let claim = status("compute").claim.clone().unwrap();
    assert_eq!((claim.worker.0, claim.phase), (1, ClaimPhase::Running));
    let labels: Vec<_> = live
        .workers
        .iter()
        .map(|w| (w.id.0, w.label.as_str(), w.online))
        .collect();
    assert_eq!(labels, [(1, "03030303", true), (2, "09090909", false)]);
    assert_eq!(live.workers[0].model, "model-3");
}

#[test]
fn failed_rows_are_rejected_candidates_and_requeue_restores_c() {
    let (s, rows) = structure();
    let failed = rust(
        &rows,
        "step",
        1,
        RustStatus::Failed,
        Validation::Unvalidated,
    );
    let live = s.live([&failed]);
    let step = live
        .status(NodeId(
            rows.iter()
                .find(|r| r.id == failed.chunk)
                .unwrap()
                .node
                .unwrap(),
        ))
        .unwrap();
    assert!(step.is_rejected() && step.claim.is_none());
    // A requeue deletes the row: the node is plain C again.
    assert_eq!(s.live([]), s.snapshot);
}

#[test]
fn only_live_claims_report_a_lease() {
    let (s, rows) = structure();
    let claimed = rust(
        &rows,
        "compute",
        1,
        RustStatus::Claimed,
        Validation::Unvalidated,
    );
    let done = rust(&rows, "step", 1, RustStatus::Done, Validation::Unvalidated);
    let live = s.live([&claimed, &done]);
    let expiry = |chunk: u64| {
        let a = live.attempts.iter().find(|a| a.job.0 == chunk).unwrap();
        a.lease_expires_ms
    };
    // `updated_at` is 2 ms in both rows.
    assert_eq!(expiry(claimed.chunk), (2_000 + LEASE_MICROS as u64) / 1000);
    assert_eq!(expiry(done.chunk), 2);
}
