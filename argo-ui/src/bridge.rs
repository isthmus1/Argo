//! Protocol → view model: one hierarchy level of an `argo-protocol` snapshot, as the canvas draws
//! it. The protocol owns meaning; this only decides presentation.
//!
//! - Nodes and wires come from [`argo_protocol::view::level`], so values only ever cross a frame
//!   through real pins and opened groups' interfaces are resolved away.
//! - The canvas draws one frame level: a node inside nested opened containers belongs to the
//!   outermost frame. Frames carry no semantics, so nothing is lost.
//! - Status drives the language chip, evidence badge and worker ring. A call site shows its
//!   callee's status: the call runs whichever implementation the manifest selects for it.
//! - A rejected candidate carries its report's failure, case count and counterexample; declared
//!   effects pass through (an empty protocol list is "not analysed", left to inference).
//! - Loop backedges (`exec.next`, `next.*`) are drawn but don't rank layout columns.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use argo_protocol::{
    AttemptId, Candidate, ClaimPhase, Failure, Family, GraphIndex, Language, NodeId, NodeKind as P,
    NodeStatus, Outcome, PinKind, Relation, Snapshot, ValueType, Verdict, view,
};

use crate::model::{
    Claim, Counterexample, Effect, Evidence, Graph, Group, Kind, Lang, Node, Pin, Rejection, Ty,
    Wire,
};

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
        P::Program | P::Module => Kind::Group,
        P::Sequence | P::Scope | P::Fragment | P::Opaque => Kind::Region,
        P::Expression => Kind::Op,
        P::Local | P::Constant | P::State => Kind::Local,
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

/// What a node shows: language, evidence, claim, the rejecting report's summary and declared
/// effects.
struct Shown {
    lang: Lang,
    evidence: Evidence,
    claim: Option<Claim>,
    rejection: Option<Rejection>,
    effects: Option<Vec<Effect>>,
}

/// The node whose status and effects a node shows: a call site shows its callee's, since the
/// call runs whichever implementation the manifest selects for it.
fn shown(snap: &Snapshot, ix: &GraphIndex, id: NodeId) -> NodeId {
    match ix.node(id).map(|n| n.kind) {
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
    }
}

fn failure(f: Failure) -> &'static str {
    match f {
        Failure::Build => "build failed",
        Failure::Mismatch => "output mismatch",
        Failure::Unsupported => "unsupported",
        Failure::Timeout => "timed out",
        Failure::ModelUnavailable => "model unavailable",
        Failure::Stale => "stale base",
    }
}

fn effect(e: &argo_protocol::Effect) -> Effect {
    use argo_protocol::Effect as E;
    match e {
        E::Pure => Effect::Pure,
        E::StateRead { .. } => Effect::StateRead,
        E::StateWrite { .. } => Effect::StateWrite,
        E::MemoryRead { .. } => Effect::MemoryRead,
        E::MemoryWrite { .. } => Effect::MemoryWrite,
        E::IndirectCall { .. } => Effect::IndirectCall,
        E::HostIo { .. } => Effect::HostIo,
        E::NonReturn => Effect::NonReturn,
        E::Unknown { .. } => Effect::Unknown,
    }
}

/// The rejected candidate's report, reduced to its failure category, case count and
/// counterexample. Before the report arrives only the verdict's category is known.
fn rejection(snap: &Snapshot, s: &NodeStatus) -> Option<Rejection> {
    let Some(Candidate { verdict: Verdict::Failed { report, failure: f }, .. }) = &s.candidate else { return None };
    let r = snap.report(*report);
    let category = match r.map(|r| &r.outcome) {
        Some(Outcome::Failed { failure: rf }) => *rf,
        _ => *f,
    };
    Some(Rejection {
        failure: failure(category),
        cases: r.filter(|r| r.cases_total > 0).map(|r| (r.cases_passed, r.cases_total)),
        counterexample: r.and_then(|r| r.counterexample.as_ref()).map(|c| Counterexample {
            input: c.input.clone(),
            expected: c.expected.clone(),
            actual: c.actual.clone(),
        }),
    })
}

/// Status and effects for a node. Call sites take their callee's.
fn status(snap: &Snapshot, ix: &GraphIndex, id: NodeId) -> Shown {
    let shown = shown(snap, ix, id);
    // An empty effect list means "not analysed yet", not pure: the card infers from shape.
    let effects = ix.node(shown).filter(|n| !n.effects.is_empty()).map(|n| n.effects.iter().map(effect).collect());
    let (lang, evidence, claim) = own_status(snap, shown);
    let rejection = snap.status(shown).and_then(|s| rejection(snap, s));
    Shown { lang, evidence, claim, rejection, effects }
}

/// Language, evidence and claim of the node itself (a function, as the explorer lists it).
pub fn own_status(snap: &Snapshot, id: NodeId) -> (Lang, Evidence, Option<Claim>) {
    let Some(s) = snap.status(id) else { return (Lang::None, Evidence::None, None) };
    let lang = match s.accepted {
        Language::None => Lang::None,
        Language::C => Lang::C,
        Language::Rust => Lang::Rust,
    };
    let evidence = if s.is_rejected() {
        Evidence::Rejected
    } else if s.is_accepted_rust() {
        Evidence::Accepted
    } else if s.candidate.as_ref().is_some_and(|c| c.language == Language::Rust && c.verdict == Verdict::Unvalidated) {
        Evidence::Unvalidated
    } else {
        Evidence::None
    };
    let claim = s.claim.as_ref().map(|c| Claim {
        worker: u8::try_from(c.worker.0).unwrap_or(u8::MAX),
        active: c.phase == ClaimPhase::Running && !lease_expired(snap, c.attempt),
    });
    (lang, evidence, claim)
}

/// Whether opening this container draws a frame: a function, module or program, or a container
/// collapsed into a merged group (§5.10). Control flow never does.
fn node_group(snap: &Snapshot, ix: &GraphIndex, id: NodeId) -> bool {
    let group = ix.node(id).is_some_and(|n| matches!(n.kind, P::Function | P::Module | P::Program));
    group || snap.status(id).is_some_and(|s| s.collapsed)
}

/// Whether a claim's attempt outlived its lease: its worker stopped renewing it (it died or lost
/// its connection), so nothing is being translated until another worker reclaims the node. A zero
/// lease is unknown (the fixture and snapshot files record none).
pub fn lease_expired(snap: &Snapshot, attempt: AttemptId) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    snap.attempts.iter().find(|a| a.id == attempt).is_some_and(|a| a.lease_expires_ms != 0 && u128::from(a.lease_expires_ms) <= now)
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

    // Frames are only for opening a node group: a function's internals, or a merged group (§5.10).
    // Control flow is never framed: an opened `for`/`if`/`switch`/block lays its contents out
    // flat, and the construct's own node (the dispatch) wires straight to them, as in UE5.
    let drawn = |f: NodeId| node_group(snap, &ix, f);
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
            let Shown { lang, evidence, claim, rejection, effects } = status(snap, &ix, f);
            Group { title: ix.node(f).map_or_else(String::new, frame_title), lang, evidence, claim, effects, rejection }
        })
        .collect();

    let mut nodes = Vec::new();
    let mut origin = Vec::new();
    let mut socket: HashMap<(NodeId, String, bool), (usize, usize)> = HashMap::new();
    for ln in &level.nodes {
        let Some(n) = ix.node(ln.id) else { continue };
        let vi = nodes.len();
        let Shown { lang, evidence, claim, rejection, effects } = status(snap, &ix, ln.id);
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
        let has_children = !ix.children(Some(ln.id)).is_empty();
        // A closed node group (a function's internals, a merged bundle) is drawn as a collapsed
        // group node, which opens in place as a frame.
        let kind = if has_children && node_group(snap, &ix, ln.id) { Kind::Group } else { kind(n.kind) };
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
            effects,
            rejection,
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

    // Each Output says what it returns, read off the `return …;` statement that ends there.
    for w in &wires {
        let (src, dst) = (w.from.0, w.to.0);
        if nodes[dst].kind == Kind::GroupOut
            && nodes[dst].ins[w.to.1].ty == Ty::Exec
            && let Some(v) = nodes[src].title.strip_prefix("return ").map(|v| v.trim_end_matches(';').trim())
        {
            nodes[dst].title = format!("Output · returns {v}");
        }
    }

    let mut crumbs: Vec<String> = container
        .into_iter()
        .flat_map(|c| std::iter::once(c).chain(ix.ancestors(c)))
        .filter_map(|id| ix.node(id).map(|n| n.name.clone()))
        .collect();
    crumbs.reverse();
    Projection {
        graph: Graph { crumbs, nodes, wires, groups, selected_group: None, expanded: None },
        origin,
        frames,
    }
}

/// The Rust source a node shows (its callee's for a call site): the accepted implementation, else
/// the latest candidate's, when its preview was recorded.
pub fn rust_source(snap: &Snapshot, id: NodeId) -> Option<&str> {
    let ix = GraphIndex::new(&snap.graph);
    let s = snap.status(shown(snap, &ix, id))?;
    let implementation = s.implementation.or(s.candidate.as_ref().map(|c| c.implementation))?;
    snap.implementations
        .iter()
        .find(|i| i.id == implementation && i.language == Language::Rust)?
        .preview
        .as_deref()
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
    fn control_flow_opens_flat_without_frames() {
        let p = compute_level(&fixture::snapshot());
        let titles: Vec<_> = p.graph.nodes.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["Input", "uint32_t acc = 0", "for (uint32_t i = 0; i < n; i++)", "step(acc, i)", "return acc", "Output · returns acc"]);
        assert!(p.graph.groups.is_empty(), "an opened loop is not a node group");
        assert!(p.graph.nodes.iter().all(|n| n.group.is_none()));
        let back: Vec<_> = p.graph.wires.iter().filter(|w| w.back).map(|w| p.graph.nodes[w.from.0].title.as_str()).collect();
        assert_eq!(back, ["step(acc, i)", "step(acc, i)"], "exec and acc both loop back into the dispatch");
        assert_eq!(p.graph.crumbs, ["fixture", "compute.c", "compute"]);
    }

    #[test]
    fn opened_functions_are_framed() {
        let (_, ids) = fixture::graph();
        let open = [ids.compute].into_iter().collect();
        let p = project(&fixture::snapshot(), Some(ids.module), &open);
        let titles: Vec<_> = p.graph.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["compute"], "a function's internals are a node group");
        assert!(p.graph.nodes.iter().any(|n| n.title == "uint32_t acc = 0" && n.group == Some(0)));
    }

    #[test]
    fn a_rejected_candidate_carries_its_counterexample() {
        // Replay up to the rejection of the wrong `step` candidate.
        let mut snap = fixture::snapshot();
        let updates = fixture::transcript();
        let upto = updates.iter().position(|u| matches!(u, argo_protocol::Update::Status(s) if s.is_rejected())).unwrap();
        for u in updates.into_iter().take(upto + 1) {
            snap.apply(u);
        }
        let p = compute_level(&snap);
        let step = p.graph.nodes.iter().find(|n| n.title == "step(acc, i)").unwrap();
        assert_eq!(step.evidence, Evidence::Rejected);
        let r = step.rejection.as_ref().expect("the report's summary");
        assert_eq!((r.failure, r.cases), ("output mismatch", Some((9, 33))));
        let cx = r.counterexample.as_ref().unwrap();
        assert_eq!((cx.input.as_str(), cx.expected.as_str(), cx.actual.as_str()), ("acc = 250, i = 10", "4", "260"));
        // Nothing else was rejected, and nothing is invented for it.
        assert!(p.graph.nodes.iter().filter(|n| n.title != "step(acc, i)").all(|n| n.rejection.is_none()));
    }

    #[test]
    fn declared_effects_pass_through_and_unanalysed_stay_unknown() {
        let p = compute_level(&fixture::snapshot());
        let step = p.graph.nodes.iter().find(|n| n.title == "step(acc, i)").unwrap();
        assert_eq!(step.effects.as_deref(), Some(&[Effect::Pure][..]), "the callee is declared pure");
        let input = p.graph.nodes.iter().find(|n| n.kind == Kind::GroupIn).unwrap();
        assert_eq!(input.effects, None, "an empty list is not analysed, not pure");
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
