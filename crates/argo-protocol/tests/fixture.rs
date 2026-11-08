use argo_protocol::{
    Graph, GraphIndex, GraphRecord, Language, Relation, RevisionState, Snapshot, fixture, validate,
};

#[test]
fn fixture_graph_is_structurally_valid() {
    let (graph, _) = fixture::graph();
    assert_eq!(validate(&graph), vec![], "fixture violations");
    assert_eq!(
        (graph.nodes.len(), graph.pins.len(), graph.edges.len()),
        (28, 84, 42)
    );
}

#[test]
fn freeze_stamps_a_stable_hash() {
    let (mut a, _) = fixture::graph();
    let (mut b, _) = fixture::graph();
    let ha = a.freeze().unwrap();
    assert_eq!(ha, b.freeze().unwrap());
    assert_eq!(a.revision.state, RevisionState::Frozen { hash: ha.clone() });
    assert_eq!(validate(&a), vec![]);
    assert_eq!(ha.as_str().len(), 64);
}

#[test]
fn hash_covers_structure_only() {
    let (mut graph, ids) = fixture::graph();
    let base = argo_protocol::canonical_hash(&graph);

    // Record order and revision identity don't matter.
    graph.nodes.reverse();
    graph.pins.reverse();
    graph.revision.id = argo_protocol::RevisionId(99);
    assert_eq!(argo_protocol::canonical_hash(&graph), base);

    // A pin label is structure.
    let pin = graph
        .pins
        .iter_mut()
        .find(|p| p.node == ids.normalize)
        .unwrap();
    pin.label.push('!');
    assert_ne!(argo_protocol::canonical_hash(&graph), base);
}

#[test]
fn tampered_frozen_hash_is_rejected() {
    let (mut graph, ids) = fixture::graph();
    graph.freeze().unwrap();
    graph
        .nodes
        .iter_mut()
        .find(|n| n.id == ids.step)
        .unwrap()
        .name = "stepp".into();
    assert!(validate(&graph).contains(&argo_protocol::Violation::HashMismatch));
}

#[test]
fn batches_reassemble_into_the_same_graph() {
    let (graph, _) = fixture::graph();
    let mut rebuilt = Graph::empty(graph.revision.clone());
    let batches = graph.batches();
    assert!(
        batches
            .iter()
            .all(|b| b.len() <= argo_protocol::revision::MAX_BATCH)
    );
    for record in batches.into_iter().flatten() {
        rebuilt.insert(record);
    }
    assert_eq!(
        argo_protocol::canonical_hash(&rebuilt),
        argo_protocol::canonical_hash(&graph)
    );
    assert!(matches!(graph.batches()[0][0], GraphRecord::File(_)));
}

#[test]
fn every_region_pin_is_explicit() {
    // The loop region reads n and carries acc; nothing else enters it.
    let (graph, ids) = fixture::graph();
    let ix = GraphIndex::new(&graph);
    let mut keys: Vec<_> = ix
        .pins(ids.loop_region)
        .map(|p| p.key.to_string())
        .collect();
    keys.sort();
    assert_eq!(keys, ["exec.in", "exec.then", "live.acc", "live.n"]);
    // The loop body sees acc and i only through its own pins (§5.6: live-in acc, i; live-out acc).
    let mut keys: Vec<_> = ix
        .pins(ids.loop_body)
        .map(|p| (p.key.to_string(), p.direction))
        .collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    use argo_protocol::Direction::*;
    assert_eq!(
        keys,
        [
            ("exec.in".into(), In),
            ("exec.then".into(), Out),
            ("live.acc".into(), InOut),
            ("live.i".into(), In)
        ]
    );
}

#[test]
fn snapshot_round_trips_through_json() {
    let snap = fixture::snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, snap);
}

#[test]
fn golden_snapshot_json() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/compute/snapshot.json"
    );
    let json = serde_json::to_string_pretty(&fixture::snapshot()).unwrap() + "\n";
    if std::env::var_os("ARGO_BLESS").is_some() {
        std::fs::write(path, &json).unwrap();
    }
    let golden =
        std::fs::read_to_string(path).expect("run with ARGO_BLESS=1 to create the snapshot");
    assert!(
        golden == json,
        "fixtures/compute/snapshot.json is stale; rerun with ARGO_BLESS=1"
    );
}

#[test]
fn transcript_reaches_the_mixed_manifest() {
    let (_, ids) = fixture::graph();
    let mut snap = fixture::snapshot();
    let mut saw_rejection = false;
    for update in fixture::transcript() {
        snap.apply(update);
        saw_rejection |= snap.status(ids.step).is_some_and(|s| s.is_rejected());
    }
    assert!(
        saw_rejection,
        "the wrong step candidate is rejected along the way"
    );
    let step = snap.status(ids.step).unwrap();
    assert!(step.is_accepted_rust() && step.candidate.is_none());
    let report = snap.report(step.evidence[0]).unwrap();
    assert_eq!((report.cases_passed, report.cases_total), (33, 33));
    assert!(snap.status(ids.normalize).unwrap().is_accepted_rust());
    assert_eq!(snap.status(ids.compute).unwrap().accepted, Language::C);
    assert!(snap.status(ids.loop_body).unwrap().claim.is_some());
    // The rejected report keeps its counterexample.
    let rejected = snap
        .reports
        .iter()
        .find(|r| r.counterexample.is_some())
        .unwrap();
    assert!(matches!(
        rejected.outcome,
        argo_protocol::Outcome::Failed { .. }
    ));
}

#[test]
fn call_relations_lift_to_functions() {
    let (graph, ids) = fixture::graph();
    let ix = GraphIndex::new(&graph);
    let mut calls: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|e| match e.relation {
            Relation::Call { site, callee } => {
                let caller = ix.enclosing(site, |n| n.kind == argo_protocol::NodeKind::Function)?;
                Some((caller, callee))
            }
            _ => None,
        })
        .collect();
    calls.sort();
    assert_eq!(calls, [(ids.step, ids.normalize), (ids.compute, ids.step)]);
}
