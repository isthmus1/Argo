//! Framework-independent graph view model and node geometry.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// The latest candidate was rejected by a counterexample.
    Rejected,
}

/// An agent's claim: which worker, and whether it is executing or queued.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub worker: u8,
    pub active: bool,
}

pub struct Pin {
    pub label: String,
    pub ty: Ty,
}

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
}

/// (from node, output index) → (to node, input index).
#[derive(Clone, Copy)]
pub struct Wire {
    pub from: (usize, usize),
    pub to: (usize, usize),
}

/// An expanded node group, drawn as a frame around its members. Frames exist only for expanded
/// groups and carry no semantics; wires connect straight to the members' pins.
pub struct Group {
    pub title: String,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
}

pub struct Graph {
    pub crumbs: Vec<&'static str>,
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    pub groups: Vec<Group>,
    pub selected_group: Option<usize>,
}

impl Graph {
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
    let title = text_w(&n.title, 6.9) + 46.0;
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
