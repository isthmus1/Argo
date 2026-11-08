//! Framework-independent graph view model and node geometry.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Ty {
    Exec,
    U32,
    Bool,
    Ptr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    /// Structural graph nodes (group I/O, locals, assignments) have no implementation language.
    None,
    C,
    Rust,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    None,
    Accepted,
    /// A Rust candidate was submitted but nothing has judged it; the C implementation stays.
    Unvalidated,
    /// The latest candidate was rejected by a counterexample.
    Rejected,
}

/// An agent's claim: which worker, and whether it is executing or queued.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub worker: u8,
    pub active: bool,
}

#[derive(Clone)]
pub struct Pin {
    pub label: String,
    pub ty: Ty,
    /// Exact type spelling as (C, Rust), when the source knows it; otherwise derived from `ty`.
    pub spelled: Option<[String; 2]>,
}

#[derive(Clone)]
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
#[derive(Clone)]
pub struct Group {
    pub title: String,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
}

#[derive(Clone)]
pub struct Graph {
    pub crumbs: Vec<String>,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    pub groups: Vec<Group>,
    pub selected_group: Option<usize>,
}

impl Graph {
    /// The view graph: each collapsed group becomes one node whose pins are the wires that
    /// cross its frame. View state only; the semantic graph is untouched.
    pub fn collapse(&self, collapsed: &[bool]) -> (Graph, Vec<Option<usize>>) {
        let n = self.nodes.len();
        let mut map: Vec<Option<usize>> = vec![None; n];
        let mut out = Graph { crumbs: self.crumbs.clone(), nodes: vec![], wires: vec![], groups: self.groups.clone(), selected_group: self.selected_group };
        // view index -> original node (None for a collapsed group node)
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
                        });
                        origin.push(None);
                        out.nodes.len() - 1
                    });
                    map[i] = Some(gi);
                }
                _ => {
                    out.nodes.push(node.clone());
                    origin.push(Some(i));
                    map[i] = Some(out.nodes.len() - 1);
                }
            }
        }
        let mut in_pins: Vec<Vec<(usize, usize)>> = vec![vec![]; out.nodes.len()];
        let mut out_pins: Vec<Vec<(usize, usize)>> = vec![vec![]; out.nodes.len()];
        for w in &self.wires {
            let (a, b) = (map[w.from.0].expect("mapped"), map[w.to.0].expect("mapped"));
            if a == b && origin[a].is_none() {
                continue; // internal to a collapsed group
            }
            let from = if origin[a].is_none() {
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
            let to = if origin[b].is_none() {
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
            out.wires.push(Wire { from, to, back: w.back });
        }
        // Exec pins first on collapsed nodes, keeping wire indices consistent.
        for (vi, node) in out.nodes.iter_mut().enumerate() {
            if origin[vi].is_some() {
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

fn text_w(s: &str, per: f32) -> f32 {
    s.chars().count() as f32 * per
}

/// Node size in points, estimated from its title and pin labels.
pub fn size(n: &Node) -> [f32; 2] {
    if n.ins.is_empty() && n.outs.len() == 1 && n.outs[0].label.is_empty() {
        // Constants and getters: a compact pill-sized node.
        let w = ((text_w(&n.title, 6.9) + 44.0).max(56.0) / 10.0).ceil() * 10.0;
        return [w, HEADER + 2.0];
    }
    // Header: padding, family glyph, title, then the language chip (and a collapsed group's
    // member-count chip), each with its gap.
    let chip = match n.lang {
        Lang::None => 0.0,
        Lang::C => 16.0 + 7.0,
        Lang::Rust => 26.0 + 7.0,
    };
    let count = if n.kind == Kind::Group { 34.0 + 7.0 } else { 0.0 };
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
