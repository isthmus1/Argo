//! Structural checks S runs before freezing a revision (§4 Freeze). Passing them means the graph
//! is well formed, not that any implementation behaves correctly.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::{
    Direction, DispatchKind, EdgeId, Family, Flow, Graph, GraphIndex, NodeId, NodeKind, PinRef,
    Relation, RevisionState, pin::PinKey,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    SchemaVersion {
        found: u32,
    },
    DuplicateNode(NodeId),
    DuplicatePin(PinRef),
    DuplicateEdge(EdgeId),
    MissingParent {
        node: NodeId,
        parent: NodeId,
    },
    ContainmentCycle(NodeId),
    NotAContainer {
        node: NodeId,
        parent: NodeId,
    },
    /// An interface or dispatch node outside a matching container.
    MisplacedSynthesized(NodeId),
    PinOnMissingNode(PinRef),
    /// A data-only kind (or impure data-only call) with exec pins, or vice versa.
    ExecPins {
        node: NodeId,
        detail: &'static str,
    },
    /// A pin with no label, or one its node's other inputs (or outputs) already use: the GUI
    /// couldn't tell them apart.
    PinLabel {
        pin: PinRef,
        detail: &'static str,
    },
    /// An opened container's group input/output doesn't mirror the container's pins.
    Interface {
        node: NodeId,
        key: Option<PinKey>,
        detail: &'static str,
    },
    MissingPin {
        edge: EdgeId,
        pin: PinRef,
    },
    MixedFamilies(EdgeId),
    WrongDirection {
        edge: EdgeId,
        pin: PinRef,
    },
    IncompatibleTypes(EdgeId),
    /// Wires join siblings only; values cross a boundary through the container's own pins.
    CrossesContainment(EdgeId),
    /// An exec output drives at most one wire.
    ExecFanOut(PinRef),
    /// A data input reads from at most one wire.
    DataFanIn(PinRef),
    /// A cycle not closed through a loop dispatch's backedge (`exec.next`, `next.*`, `cond`).
    Cycle {
        family: Family,
        node: NodeId,
    },
    BadRelation {
        edge: EdgeId,
        detail: &'static str,
    },
    /// A call site's pins don't mirror its callee's.
    CallSite {
        site: NodeId,
        key: PinKey,
    },
    WorkUnit {
        node: NodeId,
        detail: &'static str,
    },
    UnknownFile(NodeId),
    ProvenanceOutsideParent(NodeId),
    LayoutTarget(NodeId),
    HashMismatch,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub fn validate(graph: &Graph) -> Vec<Violation> {
    let ix = GraphIndex::new(graph);
    let mut v = Vec::new();
    if graph.schema_version != crate::SCHEMA_VERSION {
        v.push(Violation::SchemaVersion {
            found: graph.schema_version,
        });
    }
    identities(graph, &mut v);
    containment(&ix, &mut v);
    node_pins(&ix, &mut v);
    labels(&ix, &mut v);
    interfaces(&ix, &mut v);
    wires(&ix, &mut v);
    cycles(&ix, Family::Control, &mut v);
    cycles(&ix, Family::Data, &mut v);
    relations(&ix, &mut v);
    provenance(&ix, &mut v);
    if let RevisionState::Frozen { hash } = &graph.revision.state
        && *hash != crate::canonical_hash(graph)
    {
        v.push(Violation::HashMismatch);
    }
    v
}

fn identities(graph: &Graph, v: &mut Vec<Violation>) {
    let mut seen = HashSet::new();
    for n in &graph.nodes {
        if !seen.insert(n.id) {
            v.push(Violation::DuplicateNode(n.id));
        }
    }
    let mut pins = HashSet::new();
    for p in &graph.pins {
        if !seen.contains(&p.node) {
            v.push(Violation::PinOnMissingNode(p.pin_ref()));
        }
        if !pins.insert(p.pin_ref()) {
            v.push(Violation::DuplicatePin(p.pin_ref()));
        }
    }
    let mut edges = HashSet::new();
    for e in &graph.edges {
        if !edges.insert(e.id) {
            v.push(Violation::DuplicateEdge(e.id));
        }
    }
    for l in &graph.layouts {
        if !graph
            .nodes
            .iter()
            .any(|n| n.id == l.node && matches!(n.kind, NodeKind::Type(_)))
        {
            v.push(Violation::LayoutTarget(l.node));
        }
    }
}

fn containment(ix: &GraphIndex, v: &mut Vec<Violation>) {
    for n in &ix.graph.nodes {
        if let Some(parent) = n.parent {
            match ix.node(parent) {
                None => v.push(Violation::MissingParent { node: n.id, parent }),
                Some(p) if !p.kind.is_container() => {
                    v.push(Violation::NotAContainer { node: n.id, parent })
                }
                Some(p) => {
                    let placed = match n.kind {
                        NodeKind::GroupInput | NodeKind::GroupOutput => {
                            p.kind.has_interior_interface()
                        }
                        NodeKind::Dispatch(d) => matches!(
                            (d, p.kind),
                            (DispatchKind::Branch, NodeKind::Branch)
                                | (DispatchKind::Loop, NodeKind::Loop(_))
                                | (DispatchKind::Switch, NodeKind::Switch)
                        ),
                        _ => true,
                    };
                    if !placed {
                        v.push(Violation::MisplacedSynthesized(n.id));
                    }
                }
            }
        } else if n.kind.is_synthesized() {
            v.push(Violation::MisplacedSynthesized(n.id));
        }
        // A node on a cycle reaches itself; `ancestors` stops there.
        let mut cur = n.parent;
        let mut steps = 0;
        while let Some(p) = cur {
            if p == n.id {
                v.push(Violation::ContainmentCycle(n.id));
                break;
            }
            steps += 1;
            if steps > ix.graph.nodes.len() {
                break;
            }
            cur = ix.node(p).and_then(|x| x.parent);
        }
        match (n.kind, n.work_unit) {
            (NodeKind::Function, false) => v.push(Violation::WorkUnit {
                node: n.id,
                detail: "functions are always work units",
            }),
            (k, true)
                if k.is_synthesized() || matches!(k, NodeKind::Program | NodeKind::Module) =>
            {
                v.push(Violation::WorkUnit {
                    node: n.id,
                    detail: "structural nodes are never work units",
                })
            }
            _ => {}
        }
    }
}

fn node_pins(ix: &GraphIndex, v: &mut Vec<Violation>) {
    for n in &ix.graph.nodes {
        let pins: Vec<_> = ix.pins(n.id).collect();
        let exec: Vec<_> = pins
            .iter()
            .filter(|p| p.family() == Family::Control)
            .collect();
        let entries = exec
            .iter()
            .filter(|p| p.direction.is_input() && p.kind.flow() == Some(&Flow::Entry))
            .count();
        let mut bad = |detail| v.push(Violation::ExecPins { node: n.id, detail });
        if n.kind.is_data_only() && !exec.is_empty() {
            bad("data-only kinds have no exec pins");
        }
        if n.kind == NodeKind::Call && exec.is_empty() && !n.is_pure() {
            bad("a call may be data-only only when all its effects are pure");
        }
        if n.kind.needs_exec() && entries != 1 {
            bad("exec nodes need exactly one entry pin");
        }
        for p in &exec {
            match (p.direction, p.kind.flow()) {
                (Direction::InOut, _) => bad("exec pins are never in/out"),
                (Direction::In, Some(Flow::Backedge))
                    if n.kind != NodeKind::Dispatch(DispatchKind::Loop) =>
                {
                    bad("only a loop dispatch takes a backedge")
                }
                (Direction::In, Some(Flow::Entry | Flow::Backedge)) => {}
                (Direction::In, _) if n.kind != NodeKind::GroupOutput => {
                    bad("exec inputs are entries or backedges")
                }
                _ => {}
            }
        }
        match n.kind {
            NodeKind::GroupInput if pins.iter().any(|p| p.direction.is_input()) => {
                bad("a group input only has outputs")
            }
            NodeKind::GroupOutput => {
                if pins.iter().any(|p| p.direction.is_output()) {
                    bad("a group output only has inputs");
                }
                if exec.len() != 1 {
                    bad("a group output is exactly one exit path");
                }
            }
            _ => {}
        }
    }
}

fn labels(ix: &GraphIndex, v: &mut Vec<Violation>) {
    for n in &ix.graph.nodes {
        let mut seen: HashSet<(bool, &str)> = HashSet::new();
        for p in ix.pins(n.id) {
            if p.label.trim().is_empty() {
                v.push(Violation::PinLabel {
                    pin: p.pin_ref(),
                    detail: "every pin is labelled",
                });
                continue;
            }
            for side in [
                p.direction.is_input().then_some(false),
                p.direction.is_output().then_some(true),
            ]
            .into_iter()
            .flatten()
            {
                if !seen.insert((side, p.label.as_str())) {
                    v.push(Violation::PinLabel {
                        pin: p.pin_ref(),
                        detail: "labels are unique per side",
                    });
                }
            }
        }
    }
}

/// Opened containers: one group input mirroring every input, group outputs mirroring outputs.
fn interfaces(ix: &GraphIndex, v: &mut Vec<Violation>) {
    for c in &ix.graph.nodes {
        let kids = ix.children(Some(c.id));
        if !c.kind.has_interior_interface() || kids.is_empty() {
            continue;
        }
        let iface = |node, key: Option<&PinKey>, detail| Violation::Interface {
            node,
            key: key.cloned(),
            detail,
        };
        let outer: HashMap<_, _> = ix.pins(c.id).map(|p| (p.key.clone(), p)).collect();
        let inputs: Vec<_> = kids
            .iter()
            .filter(|&&k| ix.node(k).is_some_and(|n| n.kind == NodeKind::GroupInput))
            .collect();
        if inputs.len() != 1 {
            v.push(iface(
                c.id,
                None,
                "an opened container has exactly one group input",
            ));
        }
        for &&gi in &inputs {
            let inner: HashMap<_, _> = ix.pins(gi).map(|p| (p.key.clone(), p)).collect();
            for (k, p) in &outer {
                if p.direction.is_input() && !inner.contains_key(k) {
                    v.push(iface(gi, Some(k), "container input not mirrored"));
                }
            }
            for (k, p) in &inner {
                match outer.get(k) {
                    Some(o) if o.direction.is_input() && o.kind == p.kind => {}
                    _ => v.push(iface(
                        gi,
                        Some(k),
                        "group input pin is not a container input",
                    )),
                }
            }
        }
        for go in ix.group_outputs(c.id) {
            for p in ix.pins(go) {
                match outer.get(&p.key) {
                    Some(o) if o.direction.is_output() && o.kind == p.kind => {}
                    _ => v.push(iface(
                        go,
                        Some(&p.key),
                        "group output pin is not a container output",
                    )),
                }
            }
        }
    }
}

fn wires(ix: &GraphIndex, v: &mut Vec<Violation>) {
    let mut exec_out: HashMap<&PinRef, usize> = HashMap::new();
    let mut data_in: HashMap<&PinRef, usize> = HashMap::new();
    for e in &ix.graph.edges {
        let Some((from, to)) = e.relation.wire() else {
            continue;
        };
        let (Some(a), Some(b)) = (ix.pin(from), ix.pin(to)) else {
            for p in [from, to] {
                if ix.pin(p).is_none() {
                    v.push(Violation::MissingPin {
                        edge: e.id,
                        pin: p.clone(),
                    });
                }
            }
            continue;
        };
        let family = match e.relation {
            Relation::Exec { .. } => Family::Control,
            _ => Family::Data,
        };
        if a.family() != family || b.family() != family {
            v.push(Violation::MixedFamilies(e.id));
            continue;
        }
        if !a.direction.is_output() {
            v.push(Violation::WrongDirection {
                edge: e.id,
                pin: from.clone(),
            });
        }
        if !b.direction.is_input() {
            v.push(Violation::WrongDirection {
                edge: e.id,
                pin: to.clone(),
            });
        }
        if !a.kind.connects_to(&b.kind) {
            v.push(Violation::IncompatibleTypes(e.id));
        }
        let parent = |id| ix.node(id).map(|n| n.parent);
        if parent(from.node) != parent(to.node) {
            v.push(Violation::CrossesContainment(e.id));
        }
        match family {
            Family::Control => *exec_out.entry(from).or_default() += 1,
            Family::Data => *data_in.entry(to).or_default() += 1,
        }
    }
    let mut over = |counts: HashMap<&PinRef, usize>, make: fn(PinRef) -> Violation| {
        let mut pins: Vec<_> = counts
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(p, _)| p.clone())
            .collect();
        pins.sort();
        v.extend(pins.into_iter().map(make));
    };
    over(exec_out, Violation::ExecFanOut);
    over(data_in, Violation::DataFanIn);
}

/// Wires into these pins close a supported loop cycle.
fn is_backedge(ix: &GraphIndex, to: &PinRef) -> bool {
    let into_loop = ix
        .node(to.node)
        .is_some_and(|n| n.kind == NodeKind::Dispatch(DispatchKind::Loop));
    let k = to.key.as_str();
    into_loop
        && (k == crate::pin::key::NEXT || k == crate::pin::key::COND || k.starts_with("next."))
}

fn cycles(ix: &GraphIndex, family: Family, v: &mut Vec<Violation>) {
    let mut succ: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for e in &ix.graph.edges {
        let wire = match (&e.relation, family) {
            (Relation::Exec { from, to }, Family::Control)
            | (Relation::Data { from, to }, Family::Data) => (from, to),
            _ => continue,
        };
        if !is_backedge(ix, wire.1) {
            succ.entry(wire.0.node).or_default().push(wire.1.node);
        }
    }
    // Iterative three-colour DFS; report the first node found on each back edge.
    let mut colour: HashMap<NodeId, u8> = HashMap::new();
    let mut starts: Vec<_> = succ.keys().copied().collect();
    starts.sort();
    for start in starts {
        if colour.contains_key(&start) {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        colour.insert(start, 1);
        while let Some((node, i)) = stack.pop() {
            let next = succ.get(&node).and_then(|s| s.get(i)).copied();
            match next {
                Some(n) => {
                    stack.push((node, i + 1));
                    match colour.get(&n) {
                        None => {
                            colour.insert(n, 1);
                            stack.push((n, 0));
                        }
                        Some(1) => v.push(Violation::Cycle { family, node: n }),
                        _ => {}
                    }
                }
                None => {
                    colour.insert(node, 2);
                }
            }
        }
    }
}

fn relations(ix: &GraphIndex, v: &mut Vec<Violation>) {
    let kind = |id| ix.node(id).map(|n| n.kind);
    for e in &ix.graph.edges {
        let (a, b) = e.relation.nodes();
        if kind(a).is_none() || b.is_some_and(|b| kind(b).is_none()) {
            v.push(Violation::BadRelation {
                edge: e.id,
                detail: "endpoint node missing",
            });
            continue;
        }
        let bad = |detail| Violation::BadRelation { edge: e.id, detail };
        match &e.relation {
            Relation::Call { site, callee } => {
                if kind(*callee) != Some(NodeKind::Function) {
                    v.push(bad("a callee is a function"));
                } else if kind(*site) == Some(NodeKind::Call) {
                    call_site(ix, *site, *callee, v);
                }
            }
            Relation::TypeUse { ty, .. } if !matches!(kind(*ty), Some(NodeKind::Type(_))) => {
                v.push(bad("type use targets a type"))
            }
            Relation::StateUse { state, .. } if kind(*state) != Some(NodeKind::State) => {
                v.push(bad("state use targets a state node"))
            }
            Relation::Callback {
                target: Some(t), ..
            } if kind(*t) != Some(NodeKind::Function) => {
                v.push(bad("a callback target is a function"))
            }
            _ => {}
        }
    }
}

/// A call site mirrors its callee's data pins, and its exec pins unless it is a pure data-only call.
fn call_site(ix: &GraphIndex, site: NodeId, callee: NodeId, v: &mut Vec<Violation>) {
    let data = |n| -> HashMap<PinKey, (Direction, crate::PinKind)> {
        ix.pins(n)
            .filter(|p| p.family() == Family::Data)
            .map(|p| (p.key.clone(), (p.direction, p.kind.clone())))
            .collect()
    };
    let (s, c) = (data(site), data(callee));
    let mut keys: Vec<_> = s
        .keys()
        .chain(c.keys())
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    keys.sort();
    for k in keys {
        if s.get(&k) != c.get(&k) {
            v.push(Violation::CallSite { site, key: k });
        }
    }
}

fn provenance(ix: &GraphIndex, v: &mut Vec<Violation>) {
    let files: HashSet<_> = ix.graph.files.iter().map(|f| f.id).collect();
    for n in &ix.graph.nodes {
        let Some(p) = &n.provenance else { continue };
        if !files.contains(&p.range.file) {
            v.push(Violation::UnknownFile(n.id));
            continue;
        }
        let outer = ix
            .ancestors(n.id)
            .into_iter()
            .find_map(|a| ix.node(a).and_then(|a| a.provenance.as_ref()));
        if let Some(outer) = outer
            && outer.range.file == p.range.file
            && !outer.range.contains(p.range)
        {
            v.push(Violation::ProvenanceOutsideParent(n.id));
        }
    }
}
