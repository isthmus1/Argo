//! Framework-independent graph view model and node geometry.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Ty {
    Exec,
    U32,
    Bool,
    Ptr,
    /// The whole-program view's caller → callee links: not a value, no exec order.
    Call,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    GroupIn,
    GroupOut,
    Local,
    Loop,
    Branch,
    Switch,
    Op,
    Call,
    Set,
    /// View-only getter for a shared function input.
    Get,
    /// A collapsed node group.
    Group,
    /// A closed straight-line statement region (sequence, block, fragment, opaque).
    Region,
    /// A closed case group of a switch.
    Case,
    /// A struct, union, enum or typedef.
    Type,
}

/// Header colour families, following the parser's template kinds: every control-flow template
/// (if/else, switch/case/default, for/while/do, break/continue/return) shares one family.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Function entry and return interface.
    Interface,
    Control,
    /// Function definitions and calls, including collapsed groups.
    Function,
    Expression,
    /// Variables, declarations, assignments and getters.
    Variable,
    /// Straight-line statement regions.
    Statement,
    Type,
}

impl Category {
    /// The category in words, as the hover card names it.
    pub fn name(self) -> &'static str {
        match self {
            Category::Interface => "function in / out",
            Category::Control => "control flow",
            Category::Function => "function",
            Category::Expression => "expression",
            Category::Variable => "variable",
            Category::Statement => "statements",
            Category::Type => "type",
        }
    }
}

impl Kind {
    pub fn category(self) -> Category {
        match self {
            Kind::GroupIn | Kind::GroupOut => Category::Interface,
            Kind::Loop | Kind::Branch | Kind::Switch | Kind::Case => Category::Control,
            Kind::Call | Kind::Group => Category::Function,
            Kind::Op => Category::Expression,
            Kind::Local | Kind::Set | Kind::Get => Category::Variable,
            Kind::Region => Category::Statement,
            Kind::Type => Category::Type,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Lang {
    /// Structural graph nodes (group I/O, locals, assignments) have no implementation language.
    None,
    C,
    Rust,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Evidence {
    None,
    Accepted,
    /// A Rust candidate was written to the database but nothing has judged it: the node reads
    /// RS, and the panel shows the original C beside the Rust.
    Unvalidated,
    /// The latest candidate was rejected by a counterexample.
    Rejected,
}

/// An agent's claim: which worker, and whether it is executing or queued.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Claim {
    pub worker: u8,
    pub active: bool,
}

/// One declared effect of a node, as the protocol records it (§5.6). Only protocol data carries
/// these; mock scenes leave them out and the card falls back to effects inferred from shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Effect {
    Pure,
    StateRead,
    StateWrite,
    MemoryRead,
    MemoryWrite,
    IndirectCall,
    HostIo,
    NonReturn,
    /// Analysed, but some behaviour could not be classified.
    Unknown,
}

impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Effect::Pure => "pure",
            Effect::StateRead => "reads state",
            Effect::StateWrite => "writes state",
            Effect::MemoryRead => "memory read",
            Effect::MemoryWrite => "memory write",
            Effect::IndirectCall => "indirect call",
            Effect::HostIo => "host I/O",
            Effect::NonReturn => "never returns",
            Effect::Unknown => "unknown effect",
        }
    }
}

/// Why the latest candidate was rejected, from the report that rejected it.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Rejection {
    /// Failure category in words: `mismatch`, `build`, `timeout` …
    pub failure: &'static str,
    /// Cases passed of the cases run.
    pub cases: Option<(u32, u32)>,
    pub counterexample: Option<Counterexample>,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct Counterexample {
    pub input: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Clone, Hash)]
pub struct Pin {
    pub label: String,
    pub ty: Ty,
    /// Exact type spelling as (C, Rust), when the source knows it; otherwise derived from `ty`.
    pub spelled: Option<[String; 2]>,
}

#[derive(Clone, Hash)]
pub struct Node {
    pub kind: Kind,
    pub title: String,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
    pub outs: Vec<Pin>,
    pub ins: Vec<Pin>,
    /// The expanded group this node is drawn inside, if any.
    pub group: Option<usize>,
    /// For a collapsed group node: how many nodes it contains.
    pub members: usize,
    /// Declared effects, when the source analysed them (`None`: infer from shape).
    pub effects: Option<Vec<Effect>>,
    /// The latest rejecting report's summary, when the data carries reports.
    pub rejection: Option<Rejection>,
}

/// (from node, output index) → (to node, input index).
#[derive(Clone, Copy)]
pub struct Wire {
    pub from: (usize, usize),
    pub to: (usize, usize),
    /// A loop backedge (next iteration): drawn, but ignored when ranking layout columns.
    pub back: bool,
}

/// An expanded node group, drawn as a frame around its members. Frames exist only for expanded
/// groups and carry no semantics; wires connect straight to the members' pins.
#[derive(Clone, Hash)]
pub struct Group {
    pub title: String,
    /// Drawn with a dashed outline: a file in the whole-program view, a grouping rather than a
    /// node group that opens and closes.
    pub dashed: bool,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
    pub effects: Option<Vec<Effect>>,
    pub rejection: Option<Rejection>,
}

/// Where a view node comes from: a node of the semantic graph, or a collapsed group.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Node(usize),
    Group(usize),
}

#[derive(Clone)]
pub struct Graph {
    pub crumbs: Vec<String>,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    pub groups: Vec<Group>,
    pub selected_group: Option<usize>,
    /// For a view with collapsed groups: the graph it was projected from (all groups expanded),
    /// so layout can keep that graph's row wrapping. `None` for semantic graphs.
    pub expanded: Option<Box<Graph>>,
}

impl Graph {
    /// The view graph: each collapsed group becomes one node whose pins are the wires that
    /// cross its frame. View state only; the semantic graph is untouched.
    pub fn collapse(&self, collapsed: &[bool]) -> (Graph, Vec<Origin>) {
        let n = self.nodes.len();
        let mut map: Vec<Option<usize>> = vec![None; n];
        let folds = self.nodes.iter().any(|n| n.group.is_some_and(|g| collapsed.get(g).copied().unwrap_or(false)));
        let expanded = folds.then(|| Box::new(Graph { expanded: None, ..self.clone() }));
        let mut out = Graph { crumbs: self.crumbs.clone(), nodes: vec![], wires: vec![], groups: self.groups.clone(), selected_group: self.selected_group, expanded };
        // view index -> original node or collapsed group
        let mut origin = vec![];
        let mut group_node = vec![None; self.groups.len()];
        for (i, node) in self.nodes.iter().enumerate() {
            match node.group {
                Some(g) if collapsed.get(g).copied().unwrap_or(false) => {
                    let gi = *group_node[g].get_or_insert_with(|| {
                        let grp = &self.groups[g];
                        out.nodes.push(Node {
                            kind: Kind::Group, title: grp.title.clone(), lang: grp.lang, evidence: grp.evidence,
                            claim: grp.claim, outs: vec![], ins: vec![], group: None,
                            members: self.nodes.iter().filter(|n| n.group == Some(g)).count(),
                            effects: grp.effects.clone(), rejection: grp.rejection.clone(),
                        });
                        origin.push(Origin::Group(g));
                        out.nodes.len() - 1
                    });
                    map[i] = Some(gi);
                }
                _ => {
                    out.nodes.push(node.clone());
                    origin.push(Origin::Node(i));
                    map[i] = Some(out.nodes.len() - 1);
                }
            }
        }
        let mut in_pins: Vec<Vec<(usize, usize)>> = vec![vec![]; out.nodes.len()];
        let mut out_pins: Vec<Vec<(usize, usize)>> = vec![vec![]; out.nodes.len()];
        let folded = |v: usize| matches!(origin[v], Origin::Group(_));
        for w in &self.wires {
            let (a, b) = (map[w.from.0].expect("mapped"), map[w.to.0].expect("mapped"));
            if a == b && folded(a) {
                continue; // internal to a collapsed group
            }
            let from = if folded(a) {
                let pins = &mut out_pins[a];
                let idx = pins.iter().position(|&p| p == w.from).unwrap_or_else(|| {
                    pins.push(w.from);
                    let src = &self.nodes[w.from.0].outs[w.from.1];
                    out.nodes[a].outs.push(src.clone());
                    pins.len() - 1
                });
                (a, idx)
            } else {
                (a, w.from.1)
            };
            let to = if folded(b) {
                // One group input per external source value, however many members read it.
                let pins = &mut in_pins[b];
                let idx = pins.iter().position(|&p| p == w.from).unwrap_or_else(|| {
                    pins.push(w.from);
                    let dst = &self.nodes[w.to.0].ins[w.to.1];
                    out.nodes[b].ins.push(dst.clone());
                    pins.len() - 1
                });
                (b, idx)
            } else {
                (b, w.to.1)
            };
            // Several members reading one value share a pin, and a member's value read outside
            // shares one too: draw that view wire once, however many semantic wires it stands for.
            if !out.wires.iter().any(|v| v.from == from && v.to == to) {
                out.wires.push(Wire { from, to, back: w.back });
            }
        }
        // Exec pins first on collapsed nodes, keeping wire indices consistent.
        for (vi, node) in out.nodes.iter_mut().enumerate() {
            if matches!(origin[vi], Origin::Node(_)) {
                continue;
            }
            for (pins, output) in [(&mut node.ins, false), (&mut node.outs, true)] {
                let mut order: Vec<usize> = (0..pins.len()).collect();
                order.sort_by_key(|&i| pins[i].ty != Ty::Exec);
                let remap: Vec<usize> = { let mut r = vec![0; order.len()]; for (new, &old) in order.iter().enumerate() { r[old] = new; } r };
                *pins = order.iter().map(|&i| pins[i].clone()).collect();
                for w in &mut out.wires {
                    if output && w.from.0 == vi { w.from.1 = remap[w.from.1]; }
                    if !output && w.to.0 == vi { w.to.1 = remap[w.to.1]; }
                }
            }
        }
        (out, origin)
    }

    pub fn wire_ty(&self, w: &Wire) -> Ty {
        self.nodes[w.from.0].outs[w.from.1].ty
    }
}

pub const HEADER: f32 = 30.0;
pub const ROW: f32 = 22.0;
pub const BODY_PAD: f32 = 4.0;
/// The expand/collapse button in a group's node header or frame title.
pub const GROUP_BUTTON: f32 = 18.0;
/// That button as drawn (and hit-tested) at rest: a header budgets this much room for it.
pub const GROUP_HIT: f32 = 22.0;

/// Body rows as (output index, input index). A leading exec input and exec output share the
/// first row (UE's exec-on-top); remaining rows follow Blender: outputs, then inputs.
pub fn rows(n: &Node) -> Vec<(Option<usize>, Option<usize>)> {
    let exec_in = n.ins.first().is_some_and(|p| p.ty == Ty::Exec);
    let exec_out = n.outs.first().is_some_and(|p| p.ty == Ty::Exec);
    let mut rows = Vec::new();
    if exec_in || exec_out {
        rows.push((exec_out.then_some(0), exec_in.then_some(0)));
    }
    rows.extend((usize::from(exec_out)..n.outs.len()).map(|i| (Some(i), None)));
    rows.extend((usize::from(exec_in)..n.ins.len()).map(|i| (None, Some(i))));
    rows
}

/// Width of `s` in ems of the UI face at semibold: capitals and digits run wider than the
/// lowercase average, so all-caps constants (`MAGIC`, `-EPROTO`) are budgeted their real width.
pub fn ui_ems(s: &str) -> f32 {
    s.chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' { 0.72 } else { 0.6 }).sum()
}

fn text_w(s: &str, per: f32) -> f32 {
    s.chars().count() as f32 * per
}

/// Node size in points, estimated from its title and pin labels.
pub fn size(n: &Node) -> [f32; 2] {
    if n.ins.is_empty() && n.outs.len() == 1 && n.outs[0].label.is_empty() {
        // Constants and getters: a compact pill-sized node.
        let w = ((ui_ems(&n.title) * 11.5 + 44.0).max(56.0) / 10.0).ceil() * 10.0;
        return [w, HEADER + 2.0];
    }
    // Header: padding, family glyph, title, then the language chip (and a collapsed group's
    // member-count chip and expand button), each with its gap.
    let chip = match n.lang {
        Lang::None => 0.0,
        Lang::C => 16.0 + 7.0,
        Lang::Rust => 26.0 + 7.0,
    };
    let count = if n.kind == Kind::Group { 34.0 + 7.0 + GROUP_HIT + 7.0 } else { 0.0 };
    // Code headers are monospace (wider per character) — see `graph::header_font`.
    let per = if matches!(n.kind, Kind::GroupIn | Kind::GroupOut) { 6.9 } else { 7.7 };
    let title = 10.0 + 16.0 + 7.0 + text_w(&n.title, per) + chip + count + 10.0;
    let pins = rows(n)
        .iter()
        .map(|&(o, i)| {
            let ow = o.map_or(0.0, |o| text_w(&n.outs[o].label, 6.3));
            let iw = i.map_or(0.0, |i| text_w(&n.ins[i].label, 6.3));
            ow + iw + 44.0
        })
        .fold(0.0, f32::max);
    let w = (title.max(pins).max(100.0) / 10.0).ceil() * 10.0;
    [w, HEADER + BODY_PAD * 2.0 + rows(n).len() as f32 * ROW]
}

/// Socket centre in node-local coordinates.
pub fn socket_local(n: &Node, w: f32, output: bool, idx: usize) -> [f32; 2] {
    if n.ins.is_empty() && n.outs.len() == 1 && n.outs[0].label.is_empty() {
        return [w, (HEADER + 2.0) / 2.0];
    }
    let row = rows(n)
        .iter()
        .position(|&(o, i)| if output { o == Some(idx) } else { i == Some(idx) })
        .expect("pin is laid out");
    [if output { w } else { 0.0 }, HEADER + BODY_PAD + row as f32 * ROW + ROW / 2.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collapsed_group_draws_one_wire_per_pin_pair() {
        let g = crate::layout::with_getters(crate::scenes::scene("interp"));
        let mut collapsed = vec![false; g.groups.len()];
        collapsed[0] = true;
        let view = g.collapse(&collapsed).0;
        let mut seen = std::collections::HashSet::new();
        for w in &view.wires {
            assert!(seen.insert((w.from, w.to)), "duplicate view wire {:?} -> {:?}", w.from, w.to);
        }
        // The collapsed node still has one input per external value its members read, each wired.
        let gi = view.nodes.iter().position(|n| n.kind == Kind::Group).expect("collapsed node");
        let fed: std::collections::HashSet<usize> = view.wires.iter().filter(|w| w.to.0 == gi).map(|w| w.to.1).collect();
        assert_eq!(fed.len(), view.nodes[gi].ins.len());
        // The semantic graph is untouched: expanded again, every wire is back.
        assert_eq!(g.collapse(&vec![false; g.groups.len()]).0.wires.len(), g.wires.len());
        assert!(view.expanded.as_ref().is_some_and(|e| e.nodes.len() == g.nodes.len()));
    }
}
