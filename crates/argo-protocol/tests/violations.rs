//! One broken fixture per freeze rule: each mutation must produce exactly the violation it targets.

use argo_protocol::{
    EdgeId, Effect, Family, Graph, NodeId, PinRef, PinSpec, Relation, ValueType, Violation,
    fixture::{self, Ids},
    pin::key,
    validate,
};

fn broken(f: impl FnOnce(&mut Graph, Ids)) -> Vec<Violation> {
    let (mut graph, ids) = fixture::graph();
    f(&mut graph, ids);
    validate(&graph)
}

fn wire(graph: &mut Graph, exec: bool, from: (NodeId, &str), to: (NodeId, &str)) {
    let id = EdgeId(graph.edges.len() as u64 + 1000);
    let (from, to) = (PinRef::new(from.0, from.1), PinRef::new(to.0, to.1));
    let relation = if exec {
        Relation::Exec { from, to }
    } else {
        Relation::Data { from, to }
    };
    graph.edges.push(argo_protocol::Edge { id, relation });
}

fn has(v: &[Violation], pred: impl Fn(&Violation) -> bool) -> bool {
    v.iter().any(pred)
}

#[test]
fn values_cannot_skip_a_boundary() {
    // Reading compute's parameter straight from inside the loop body: the "inherited value" bug.
    let v = broken(|g, ids| {
        let gi = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.compute) && n.name == "Input")
            .unwrap()
            .id;
        wire(
            g,
            false,
            (gi, &key::param("n")),
            (ids.call_step, &key::param("i")),
        );
    });
    assert!(
        has(&v, |x| matches!(x, Violation::CrossesContainment(_))),
        "{v:?}"
    );
}

#[test]
fn data_only_kinds_have_no_exec_pins() {
    let v = broken(|g, ids| {
        let sum = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.step_body) && n.name == "acc + i")
            .unwrap()
            .id;
        g.pins.push(PinSpec::enter().at(sum));
    });
    assert!(
        has(&v, |x| matches!(x, Violation::ExecPins { .. })),
        "{v:?}"
    );
}

#[test]
fn impure_calls_stay_on_the_exec_chain() {
    let ids_call = fixture::graph().1.call_normalize;
    let v = broken(|g, ids| {
        g.nodes
            .iter_mut()
            .find(|n| n.id == ids.call_normalize)
            .unwrap()
            .effects = vec![Effect::HostIo {
            resource: "stdout".into(),
        }];
    });
    assert!(
        has(
            &v,
            |x| matches!(x, Violation::ExecPins { node, .. } if *node == ids_call)
        ),
        "{v:?}"
    );
}

#[test]
fn group_input_mirrors_every_container_input() {
    let v = broken(|g, ids| {
        let gi = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.loop_region) && n.name == "Input")
            .unwrap()
            .id;
        g.pins
            .retain(|p| !(p.node == gi && p.key.as_str() == "live.n"));
        // Its wire now dangles too; drop it to isolate the interface rule.
        g.edges.retain(|e| {
            e.relation
                .wire()
                .is_none_or(|(f, _)| !(f.node == gi && f.key.as_str() == "live.n"))
        });
    });
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(matches!(&v[0], Violation::Interface { key: Some(k), .. } if k.as_str() == "live.n"));
}

#[test]
fn group_output_only_carries_container_outputs() {
    let v = broken(|g, ids| {
        let go = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.loop_region) && n.name == "Output")
            .unwrap()
            .id;
        g.pins
            .push(PinSpec::live("n", argo_protocol::Direction::In, ValueType::u32()).at(go));
    });
    assert!(
        has(&v, |x| matches!(x, Violation::Interface { .. })),
        "{v:?}"
    );
}

#[test]
fn a_data_input_has_one_source() {
    let v = broken(|g, ids| {
        let init = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.compute) && n.name == "uint32_t acc = 0")
            .unwrap()
            .id;
        let ret = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.compute) && n.name == "return acc")
            .unwrap()
            .id;
        wire(
            g,
            false,
            (init, &key::live("acc")),
            (ret, &key::live("acc")),
        );
    });
    assert!(has(&v, |x| matches!(x, Violation::DataFanIn(_))), "{v:?}");
}

#[test]
fn an_exec_output_drives_one_wire() {
    let v = broken(|g, ids| {
        let init = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.compute) && n.name == "uint32_t acc = 0")
            .unwrap()
            .id;
        let ret = g
            .nodes
            .iter()
            .find(|n| n.parent == Some(ids.compute) && n.name == "return acc")
            .unwrap()
            .id;
        wire(g, true, (init, key::THEN), (ret, key::ENTER));
    });
    assert!(has(&v, |x| matches!(x, Violation::ExecFanOut(_))), "{v:?}");
}

#[test]
fn families_never_mix() {
    let v = broken(|g, ids| {
        wire(
            g,
            true,
            (ids.loop_dispatch, &key::live("i")),
            (ids.loop_body, &key::live("i")),
        );
    });
    assert!(
        has(&v, |x| matches!(x, Violation::MixedFamilies(_))),
        "{v:?}"
    );
}

#[test]
fn only_loop_backedges_close_cycles() {
    // The fixture's own loop cycle is fine (it validates); a cycle into a non-backedge pin isn't.
    let v = broken(|g, ids| {
        g.edges.retain(|e| {
            e.relation
                .wire()
                .is_none_or(|(_, t)| !(t.node == ids.loop_dispatch && t.key.as_str() == "next.acc"))
        });
        wire(
            g,
            false,
            (ids.loop_body, &key::live("acc")),
            (ids.loop_dispatch, &key::init("acc")),
        );
    });
    assert!(
        has(&v, |x| matches!(
            x,
            Violation::Cycle {
                family: Family::Data,
                ..
            }
        )),
        "{v:?}"
    );
}

#[test]
fn call_sites_mirror_their_callee() {
    let v = broken(|g, ids| {
        g.pins
            .retain(|p| !(p.node == ids.call_step && p.key.as_str() == "param.i"));
        g.edges.retain(|e| {
            e.relation
                .wire()
                .is_none_or(|(_, t)| !(t.node == ids.call_step && t.key.as_str() == "param.i"))
        });
    });
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(matches!(&v[0], Violation::CallSite { key, .. } if key.as_str() == "param.i"));
}

#[test]
fn functions_are_work_units() {
    let v = broken(|g, ids| {
        g.nodes
            .iter_mut()
            .find(|n| n.id == ids.step)
            .unwrap()
            .work_unit = false
    });
    assert!(
        has(&v, |x| matches!(x, Violation::WorkUnit { .. })),
        "{v:?}"
    );
}

#[test]
fn types_must_match_across_a_wire() {
    let v = broken(|g, ids| {
        let pin = g
            .pins
            .iter_mut()
            .find(|p| p.node == ids.call_step && p.key.as_str() == "param.i")
            .unwrap();
        pin.kind = argo_protocol::PinKind::Value {
            ty: ValueType::Bool,
        };
    });
    assert!(
        has(&v, |x| matches!(x, Violation::IncompatibleTypes(_))),
        "{v:?}"
    );
}

#[test]
fn wires_need_existing_pins_and_directions() {
    let v = broken(|g, ids| {
        wire(
            g,
            false,
            (ids.loop_body, "live.nope"),
            (ids.loop_dispatch, &key::next("acc")),
        );
        wire(
            g,
            true,
            (ids.loop_body, key::ENTER),
            (ids.loop_dispatch, key::NEXT),
        );
    });
    assert!(
        has(&v, |x| matches!(x, Violation::MissingPin { .. })),
        "{v:?}"
    );
    assert!(
        has(&v, |x| matches!(x, Violation::WrongDirection { .. })),
        "{v:?}"
    );
}

#[test]
fn relations_target_the_right_kinds() {
    let v = broken(|g, ids| {
        let id = EdgeId(5000);
        g.edges.push(argo_protocol::Edge {
            id,
            relation: Relation::TypeUse {
                user: ids.step,
                ty: ids.normalize,
            },
        });
    });
    assert!(
        has(&v, |x| matches!(x, Violation::BadRelation { .. })),
        "{v:?}"
    );
}

#[test]
fn containment_is_a_tree_of_containers() {
    let v = broken(|g, ids| {
        g.nodes
            .iter_mut()
            .find(|n| n.id == ids.program)
            .unwrap()
            .parent = Some(ids.compute);
    });
    assert!(
        has(&v, |x| matches!(x, Violation::ContainmentCycle(_))),
        "{v:?}"
    );
    let v = broken(|g, ids| {
        g.nodes
            .iter_mut()
            .find(|n| n.id == ids.loop_body)
            .unwrap()
            .parent = Some(ids.call_step)
    });
    assert!(
        has(&v, |x| matches!(x, Violation::NotAContainer { .. })),
        "{v:?}"
    );
}

#[test]
fn provenance_nests_inside_the_parent() {
    let v = broken(|g, ids| {
        let n = g.nodes.iter_mut().find(|n| n.id == ids.loop_body).unwrap();
        n.provenance.as_mut().unwrap().range.lo = 0;
    });
    assert!(
        has(&v, |x| matches!(x, Violation::ProvenanceOutsideParent(_))),
        "{v:?}"
    );
}

#[test]
fn every_pin_has_a_distinct_label() {
    let v = broken(|g, ids| {
        let pin = g
            .pins
            .iter_mut()
            .find(|p| p.node == ids.call_step && p.key.as_str() == "param.i")
            .unwrap();
        pin.label.clear();
    });
    assert!(
        has(
            &v,
            |x| matches!(x, Violation::PinLabel { detail, .. } if detail.contains("labelled"))
        ),
        "{v:?}"
    );
    let v = broken(|g, ids| {
        let pin = g
            .pins
            .iter_mut()
            .find(|p| p.node == ids.call_step && p.key.as_str() == "param.i")
            .unwrap();
        pin.label = "acc".into();
    });
    assert!(
        has(
            &v,
            |x| matches!(x, Violation::PinLabel { detail, .. } if detail.contains("unique"))
        ),
        "{v:?}"
    );
}
