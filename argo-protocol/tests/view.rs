use std::collections::HashSet;

use argo_protocol::{
    Family, GraphIndex, NodeId, PinRef, fixture,
    view::{LevelWire, level},
};

fn names(ix: &GraphIndex, ids: impl IntoIterator<Item = NodeId>) -> Vec<String> {
    ids.into_iter()
        .map(|id| ix.node(id).unwrap().name.clone())
        .collect()
}

fn wire(ix: &GraphIndex, w: &LevelWire) -> String {
    let n = |p: &PinRef| format!("{}.{}", ix.node(p.node).unwrap().name, p.key);
    format!("{} -> {}", n(&w.from), n(&w.to))
}

#[test]
fn closed_level_shows_region_pins() {
    let (graph, ids) = fixture::graph();
    let ix = GraphIndex::new(&graph);
    let lv = level(&ix, Some(ids.compute), |_| false);
    assert_eq!(
        names(&ix, lv.nodes.iter().map(|n| n.id)),
        [
            "Input",
            "uint32_t acc = 0",
            "for (uint32_t i = 0; i < n; i++)",
            "return acc",
            "Output"
        ]
    );
    let wires: HashSet<_> = lv.wires.iter().map(|w| wire(&ix, w)).collect();
    assert!(
        wires.contains("uint32_t acc = 0.live.acc -> for (uint32_t i = 0; i < n; i++).live.acc")
    );
    assert!(wires.contains("Input.param.n -> for (uint32_t i = 0; i < n; i++).live.n"));
    assert!(wires.contains("for (uint32_t i = 0; i < n; i++).exec.then -> return acc.exec.in"));
    assert!(lv.frames.is_empty());
}

#[test]
fn opened_regions_resolve_wires_through_their_interfaces() {
    let (graph, ids) = fixture::graph();
    let ix = GraphIndex::new(&graph);
    let open = [ids.loop_region, ids.loop_body];
    let lv = level(&ix, Some(ids.compute), |id| open.contains(&id));

    let shown = names(&ix, lv.nodes.iter().map(|n| n.id));
    assert_eq!(
        shown,
        [
            "Input",
            "uint32_t acc = 0",
            "for (uint32_t i = 0; i < n; i++)",
            "step(acc, i)",
            "return acc",
            "Output"
        ]
    );
    assert_eq!(lv.frames.len(), 2);
    let step = lv.nodes.iter().find(|n| n.id == ids.call_step).unwrap();
    assert_eq!(step.frame, Some(ids.loop_body));

    let wires: HashSet<_> = lv.wires.iter().map(|w| wire(&ix, w)).collect();
    for expected in [
        "Input.param.n -> for (uint32_t i = 0; i < n; i++).live.n",
        "uint32_t acc = 0.live.acc -> for (uint32_t i = 0; i < n; i++).init.acc",
        "uint32_t acc = 0.exec.then -> for (uint32_t i = 0; i < n; i++).exec.in",
        "for (uint32_t i = 0; i < n; i++).body -> step(acc, i).exec.in",
        "for (uint32_t i = 0; i < n; i++).live.acc -> step(acc, i).param.acc",
        "for (uint32_t i = 0; i < n; i++).live.i -> step(acc, i).param.i",
        "step(acc, i).return -> for (uint32_t i = 0; i < n; i++).next.acc",
        "step(acc, i).exec.then -> for (uint32_t i = 0; i < n; i++).exec.next",
        "for (uint32_t i = 0; i < n; i++).completed -> return acc.exec.in",
        "for (uint32_t i = 0; i < n; i++).live.acc -> return acc.live.acc",
    ] {
        assert!(
            wires.contains(expected),
            "missing {expected}; have {wires:#?}"
        );
    }
    // Nothing points at a hidden interface node or an opened container.
    let hidden: HashSet<_> = lv.frames.iter().map(|f| f.id).collect();
    assert!(
        lv.wires
            .iter()
            .all(|w| !hidden.contains(&w.from.node) && !hidden.contains(&w.to.node))
    );
    assert!(
        lv.wires
            .iter()
            .filter(|w| w.family == Family::Control)
            .count()
            >= 5
    );
}

#[test]
fn pass_through_exec_reaches_the_container_output() {
    // step's sequence has no exec nodes inside; opened, its entry flows straight to the exit.
    let (graph, ids) = fixture::graph();
    let ix = GraphIndex::new(&graph);
    let lv = level(&ix, Some(ids.step), |id| id == ids.step_body);
    let wires: HashSet<_> = lv.wires.iter().map(|w| wire(&ix, w)).collect();
    assert!(
        wires.contains("Input.exec.in -> Output.exec.then"),
        "{wires:#?}"
    );
    assert!(
        wires.contains("normalize(acc + i).return -> Output.return"),
        "{wires:#?}"
    );
    assert!(
        wires.contains("Input.param.acc -> acc + i.operand.0"),
        "{wires:#?}"
    );
}
