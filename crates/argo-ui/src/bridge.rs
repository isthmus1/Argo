//! Protocol → view model: one hierarchy level of an `argo-protocol` snapshot, as the canvas draws
//! it. The protocol owns meaning; this only decides presentation.
//!
//! - Nodes and wires come from [`argo_protocol::view::level`], so values only ever cross a frame
//!   through real pins and opened groups' interfaces are resolved away.
//! - The canvas draws one frame level: a node inside nested opened containers belongs to the
//!   outermost frame. Frames carry no semantics, so nothing is lost.
//! - Status drives the language chip, evidence badge and worker ring. A call site shows its
//!   callee's status: the call runs whichever implementation the manifest selects for it.
//! - Loop backedges (`exec.next`, `next.*`) are drawn but don't rank layout columns.

use std::collections::{HashMap, HashSet};

use argo_protocol::{
    ClaimPhase, Family, GraphIndex, Language, NodeId, NodeKind as P, PinKind, Relation,
    Snapshot, ValueType, view,
};

use crate::model::{Claim, Evidence, Graph, Group, Kind, Lang, Node, Pin, Ty, Wire};

pub struct Projection {
    pub graph: Graph,
    /// View node index → protocol node, before getters are appended.
    pub origin: Vec<NodeId>,
    /// View group index → the opened protocol container it frames.
    pub frames: Vec<NodeId>,
}

fn kind(k: P) -> Kind {
    match k {
        P::GroupInput => Kind::GroupIn,
        P::GroupOutput => Kind::GroupOut,
        P::Loop(_) | P::Dispatch(argo_protocol::DispatchKind::Loop) => Kind::Loop,
        P::Branch | P::Dispatch(argo_protocol::DispatchKind::Branch) => Kind::Branch,
        P::Switch | P::Dispatch(argo_protocol::DispatchKind::Switch) => Kind::Switch,
        P::CaseGroup => Kind::Case,
        P::Function | P::Call => Kind::Call,
        P::Program | P::Module | P::MacroGroup => Kind::Group,
        P::Sequence | P::Scope | P::Fragment | P::Opaque => Kind::Region,
        P::Expression => Kind::Op,
        P::Local | P::Constant | P::State | P::Macro => Kind::Local,
        P::Assign => Kind::Set,
        P::Type(_) => Kind::Type,
    }
}

/// An opened construct's frame says what kind of construct it is; the dispatch node inside
/// carries the header (`for (…)`, `if (…)`), so the two never repeat each other.
fn frame_title(n: &argo_protocol::Node) -> String {
    use argo_protocol::LoopKind;
    match n.kind {
        P::Loop(LoopKind::For) => "for loop".into(),
        P::Loop(LoopKind::While) => "while loop".into(),
        P::Loop(LoopKind::DoWhile) => "do … while loop".into(),
        P::Branch if n.name.ends_with("… else") => "if … else".into(),
        P::Branch => "if".into(),
        P::Switch => "switch".into(),
        _ => n.name.clone(),
    }
}

fn ty(k: &PinKind) -> Ty {
    match k {
        PinKind::Exec { .. } => Ty::Exec,
        PinKind::Value { ty: ValueType::Bool } => Ty::Bool,
        PinKind::Value { ty: ValueType::Pointer { .. } }
        | PinKind::Memory { .. }
        | PinKind::State { .. }
        | PinKind::Callback { .. } => Ty::Ptr,
        PinKind::Value { .. } => Ty::U32,
    }
}

fn spelled(k: &PinKind) -> Option<[String; 2]> {
    match k {
        PinKind::Exec { .. } => None,
        PinKind::Value { ty } => Some([ty.c(), ty.rust()]),
        PinKind::Memory { pointee, .. } => Some([format!("{} *", pointee.c()), format!("*mut {}", pointee.rust())]),
        PinKind::State { .. } => Some(["state".into(), "state".into()]),
        PinKind::Callback { .. } => Some(["fn *".into(), "fn".into()]),
    }
}

/// Language, evidence and claim for a node. Call sites take their callee's.
fn status(snap: &Snapshot, ix: &GraphIndex, id: NodeId) -> (Lang, Evidence, Option<Claim>) {
    let shown = match ix.node(id).map(|n| n.kind) {
        Some(P::Call) => snap
            .graph
            .edges
            .iter()
            .find_map(|e| match e.relation {
                Relation::Call { site, callee } if site == id => Some(callee),
                _ => None,
            })
            .unwrap_or(id),
        _ => id,
    };
    let Some(s) = snap.status(shown) else { return (Lang::None, Evidence::None, None) };
    let lang = match s.accepted {
        Language::None => Lang::None,
        Language::C => Lang::C,
        Language::Rust => Lang::Rust,
    };
    let evidence = if s.is_rejected() {
        Evidence::Rejected
    } else if s.is_accepted_rust() {
        Evidence::Accepted
    } else {
        Evidence::None
    };
    let claim = s.claim.as_ref().map(|c| Claim {
        worker: u8::try_from(c.worker.0).unwrap_or(u8::MAX),
        active: c.phase == ClaimPhase::Running,
    });
    (lang, evidence, claim)
}

fn is_backedge(ix: &GraphIndex, to: &argo_protocol::PinRef) -> bool {
    let k = to.key.as_str();
    ix.node(to.node).is_some_and(|n| n.kind == P::Dispatch(argo_protocol::DispatchKind::Loop))
        && (k == argo_protocol::pin::key::NEXT || k.starts_with("next."))
}

/// Project the level inside `container` (`None`: the roots), with `open` containers drawn in place.
pub fn project(snap: &Snapshot, container: Option<NodeId>, open: &HashSet<NodeId>) -> Projection {
    let ix = GraphIndex::new(&snap.graph);
    let level = view::level(&ix, container, |id| open.contains(&id));

    // One frame level: every node joins its outermost drawn frame. A function's body block draws
    // no frame: it is the whole function, already named by the breadcrumb.
    let drawn = |f: NodeId| {
        let n = ix.node(f);
        let body = n.is_some_and(|n| n.kind == P::Scope)
            && n.and_then(|n| n.parent).and_then(|p| ix.node(p)).is_some_and(|p| p.kind == P::Function);
        !body
    };
    let outermost: HashMap<NodeId, Option<NodeId>> = {
        let parent: HashMap<_, _> = level.frames.iter().map(|f| (f.id, f.parent)).collect();
        level
            .frames
            .iter()
            .map(|f| {
                let (mut cur, mut top) = (Some(f.id), None);
                while let Some(c) = cur {
                    if drawn(c) {
                        top = Some(c);
                    }
                    cur = parent.get(&c).copied().flatten();
                }
                (f.id, top)
            })
            .collect()
    };
    let mut frames: Vec<NodeId> = Vec::new();
    for f in &level.frames {
        if let Some(top) = outermost[&f.id]
            && !frames.contains(&top)
        {
            frames.push(top);
        }
    }
    let groups = frames
        .iter()
        .map(|&f| {
            let (lang, evidence, claim) = status(snap, &ix, f);
            Group { title: ix.node(f).map_or_else(String::new, frame_title), lang, evidence, claim }
        })
        .collect();

    let mut nodes = Vec::new();
    let mut origin = Vec::new();
    let mut socket: HashMap<(NodeId, String, bool), (usize, usize)> = HashMap::new();
    for ln in &level.nodes {
        let Some(n) = ix.node(ln.id) else { continue };
        let vi = nodes.len();
        let (lang, evidence, claim) = status(snap, &ix, ln.id);
        // Exec pins first, so a leading exec input and output share the top row.
        let mut pins: Vec<_> = ix.pins(ln.id).collect();
        pins.sort_by_key(|p| p.family() != Family::Control);
        let (mut ins, mut outs) = (Vec::new(), Vec::new());
        for p in pins {
            let label = if p.label.trim().is_empty() {
                argo_protocol::pin::label_for_key(p.key.as_str())
            } else {
                p.label.clone()
            };
            let pin = Pin { label, ty: ty(&p.kind), spelled: spelled(&p.kind) };
            if p.direction.is_input() {
                socket.insert((ln.id, p.key.to_string(), false), (vi, ins.len()));
                ins.push(pin.clone());
            }
            if p.direction.is_output() {
                socket.insert((ln.id, p.key.to_string(), true), (vi, outs.len()));
                outs.push(pin);
            }
        }
        let kind = kind(n.kind);
        let has_children = !ix.children(Some(ln.id)).is_empty();
        let group = ln.frame.and_then(|f| outermost[&f]).and_then(|top| frames.iter().position(|&g| g == top));
        nodes.push(Node {
            kind,
            title: n.name.clone(),
            lang,
            evidence,
            claim,
            ins,
            outs,
            group,
            members: if kind == Kind::Group && has_children { ix.member_count(ln.id) } else { 0 },
        });
        origin.push(ln.id);
    }
    let mut wires: Vec<Wire> = level
        .wires
        .iter()
        .filter_map(|w| {
            let from = *socket.get(&(w.from.node, w.from.key.to_string(), true))?;
            let to = *socket.get(&(w.to.node, w.to.key.to_string(), false))?;
            Some(Wire { from, to, back: is_backedge(&ix, &w.to) })
        })
        .collect();

    // One Output per exit path, like UE5's repeated Return nodes: each `return` ends right next to
    // the statement that returns instead of in one long merge. The protocol node is shared.
    for vi in 0..nodes.len() {
        if nodes[vi].kind != Kind::GroupOut {
            continue;
        }
        let exec_in: Vec<usize> = (0..wires.len())
            .filter(|&w| wires[w].to.0 == vi && nodes[vi].ins[wires[w].to.1].ty == Ty::Exec)
            .collect();
        for &wi in exec_in.iter().skip(1) {
            let src = wires[wi].from.0;
            nodes.push(nodes[vi].clone());
            origin.push(origin[vi]);
            let ci = nodes.len() - 1;
            wires[wi].to.0 = ci;
            for w in wires.iter_mut() {
                if w.to.0 == vi && w.from.0 == src && nodes[vi].ins[w.to.1].ty != Ty::Exec {
                    w.to.0 = ci;
                }
            }
        }
    }

    let mut crumbs: Vec<String> = container
        .into_iter()
        .flat_map(|c| std::iter::once(c).chain(ix.ancestors(c)))
        .filter_map(|id| ix.node(id).map(|n| n.name.clone()))
        .collect();
    crumbs.reverse();
    Projection {
        graph: Graph { crumbs, nodes, wires, groups, selected_group: None },
        origin,
        frames,
    }
}

/// Whether a protocol node can be opened in place.
pub fn openable(snap: &Snapshot, id: NodeId) -> bool {
    snap.graph.nodes.iter().any(|n| n.parent == Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use argo_protocol::fixture;

    fn compute_level(snap: &Snapshot) -> Projection {
        let (_, ids) = fixture::graph();
        let open = [ids.loop_region, ids.loop_body].into_iter().collect();
        project(snap, Some(ids.compute), &open)
    }

    #[test]
    fn projects_one_frame_with_backedges_flagged() {
        let p = compute_level(&fixture::snapshot());
        let titles: Vec<_> = p.graph.nodes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["Input", "uint32_t acc = 0", "for (uint32_t i = 0; i < n; i++)", "step(acc, i)", "return acc", "Output"]);
        assert_eq!(p.graph.groups.len(), 1, "nested frames draw as one");
        assert_eq!(p.graph.groups[0].title, "for loop");
        assert!(p.graph.nodes.iter().filter(|n| n.group == Some(0)).count() == 2);
        let back: Vec<_> = p.graph.wires.iter().filter(|w| w.back).map(|w| p.graph.nodes[w.from.0].title.as_str()).collect();
        assert_eq!(back, ["step(acc, i)", "step(acc, i)"], "exec and acc both loop back into the dispatch");
        assert_eq!(p.graph.crumbs, ["fixture", "compute.c", "compute"]);
    }

    #[test]
    fn call_sites_show_their_callee_status() {
        let mut snap = fixture::snapshot();
        for u in fixture::transcript() {
            snap.apply(u);
        }
        let p = compute_level(&snap);
        let step = p.graph.nodes.iter().find(|n| n.title == "step(acc, i)").unwrap();
        assert_eq!((step.lang, step.evidence), (Lang::Rust, Evidence::Accepted));
        let pin = &step.ins.iter().find(|p| p.label == "acc").unwrap();
        assert_eq!(pin.spelled.as_ref().unwrap(), &["uint32_t".to_string(), "u32".to_string()]);
    }
}
