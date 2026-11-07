//! Seeded mock scene for design renders. Not protocol data: it mirrors the worked fixture in
//! ARCHITECTURE.md §5.2 (`compute → step → normalize`) with the function exploded into its body,
//! Blender-style group input/output nodes, a UE-style loop whose outer-scope values enter as
//! explicit pins, and the merged Rust group `step ∘ normalize` expanded in a frame. `report` is a
//! mock-only C call used to show an agent retrying a rejected candidate.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ty {
    Exec,
    U32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    GroupIn,
    GroupOut,
    Local,
    Loop,
    Op,
    Call,
    Set,
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

/// An agent's claim on a node: which worker, and whether it is executing or queued.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub worker: u8,
    pub active: bool,
}

pub struct Pin {
    pub label: &'static str,
    pub ty: Ty,
}

pub struct Node {
    pub kind: Kind,
    pub title: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
    pub outs: &'static [Pin],
    pub ins: &'static [Pin],
}

/// (from node, output index) → (to node, input index).
pub struct Wire {
    pub from: (usize, usize),
    pub to: (usize, usize),
}

const fn p(label: &'static str, ty: Ty) -> Pin {
    Pin { label, ty }
}

use Ty::{Exec, U32};

pub const NODES: &[Node] = &[
    // 0
    Node { kind: Kind::GroupIn, title: "Input", x: 0.0, y: 40.0, w: 120.0, lang: Lang::None, evidence: Evidence::None, claim: None,
        outs: &[p("", Exec), p("n", U32)], ins: &[] },
    // 1: pure
    Node { kind: Kind::Local, title: "acc = 0", x: 0.0, y: 170.0, w: 120.0, lang: Lang::None, evidence: Evidence::None, claim: None,
        outs: &[p("acc", U32)], ins: &[] },
    // 2: outer-scope values enter only through explicit pins
    Node { kind: Kind::Loop, title: "for i in 0..n", x: 180.0, y: 30.0, w: 160.0, lang: Lang::C, evidence: Evidence::None, claim: None,
        outs: &[p("loop body", Exec), p("index", U32), p("acc", U32), p("completed", Exec), p("acc", U32)],
        ins: &[p("", Exec), p("n", U32), p("acc", U32)] },
    // 3: pure, inside the expanded `step ∘ normalize` group
    Node { kind: Kind::Op, title: "acc + i", x: 425.0, y: -105.0, w: 110.0, lang: Lang::Rust, evidence: Evidence::None, claim: None,
        outs: &[p("sum", U32)], ins: &[p("acc", U32), p("i", U32)] },
    // 4: pure, inside the expanded group
    Node { kind: Kind::Call, title: "normalize", x: 575.0, y: -105.0, w: 120.0, lang: Lang::Rust, evidence: Evidence::None, claim: None,
        outs: &[p("result", U32)], ins: &[p("x", U32)] },
    // 5
    Node { kind: Kind::Set, title: "Set acc", x: 770.0, y: 40.0, w: 110.0, lang: Lang::None, evidence: Evidence::None, claim: None,
        outs: &[p("", Exec), p("acc", U32)], ins: &[p("", Exec), p("value", U32)] },
    // 6
    Node { kind: Kind::Call, title: "report", x: 930.0, y: 40.0, w: 120.0, lang: Lang::C,
        evidence: Evidence::Rejected, claim: Some(Claim { worker: 1, active: true }),
        outs: &[p("", Exec)], ins: &[p("", Exec), p("acc", U32)] },
    // 7
    Node { kind: Kind::GroupOut, title: "Output", x: 770.0, y: 230.0, w: 120.0, lang: Lang::None, evidence: Evidence::None, claim: None,
        outs: &[], ins: &[p("", Exec), p("acc", U32)] },
];

pub const WIRES: &[Wire] = &[
    Wire { from: (0, 0), to: (2, 0) },
    Wire { from: (0, 1), to: (2, 1) },
    Wire { from: (1, 0), to: (2, 2) },
    Wire { from: (2, 0), to: (5, 0) },
    Wire { from: (2, 2), to: (3, 0) },
    Wire { from: (2, 1), to: (3, 1) },
    Wire { from: (3, 0), to: (4, 0) },
    Wire { from: (4, 0), to: (5, 1) },
    Wire { from: (5, 0), to: (6, 0) },
    Wire { from: (5, 1), to: (6, 1) },
    Wire { from: (2, 3), to: (7, 0) },
    Wire { from: (2, 4), to: (7, 1) },
];

/// An expanded node group drawn as a frame around its internal nodes. Frames exist only for
/// expanded groups (a merged bundle or a function's internals); they carry no semantics of
/// their own, and wires connect straight to the internal nodes' pins.
pub struct Frame {
    pub title: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub lang: Lang,
    pub evidence: Evidence,
    pub claim: Option<Claim>,
}

pub const FRAMES: &[Frame] = &[Frame {
    title: "step ∘ normalize",
    x: 405.0, y: -150.0, w: 310.0, h: 165.0,
    lang: Lang::Rust, evidence: Evidence::Accepted, claim: Some(Claim { worker: 2, active: false }),
}];

/// The selected item in the mock: the expanded group.
pub const SELECTED_FRAME: usize = 0;

pub const RUST_SOURCE: &[&str] = &[
    "pub fn step(acc: u32, i: u32) -> u32 {",
    "    normalize(acc.wrapping_add(i))",
    "}",
    "",
    "fn normalize(x: u32) -> u32 {",
    "    x & 0xff",
    "}",
];

/// Graph geometry shared by nodes, sockets and wires.
pub const HEADER: f32 = 30.0;
pub const ROW: f32 = 22.0;
pub const BODY_PAD: f32 = 4.0;

/// Body rows as (output index, input index). A leading exec input and exec output share the
/// first row (UE's exec-on-top); remaining rows follow Blender: outputs, then inputs.
pub fn layout(n: &Node) -> Vec<(Option<usize>, Option<usize>)> {
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

/// Socket centre in node-local coordinates.
pub fn socket_local(n: &Node, output: bool, idx: usize) -> [f32; 2] {
    let row = layout(n)
        .iter()
        .position(|&(o, i)| if output { o == Some(idx) } else { i == Some(idx) })
        .expect("pin is laid out");
    let y = HEADER + BODY_PAD + row as f32 * ROW + ROW / 2.0;
    [if output { n.w } else { 0.0 }, y]
}

pub fn socket_world(i: usize, output: bool, idx: usize) -> [f32; 2] {
    let n = &NODES[i];
    let [x, y] = socket_local(n, output, idx);
    [n.x + x, n.y + y]
}

pub fn wire_ty(w: &Wire) -> Ty {
    NODES[w.from.0].outs[w.from.1].ty
}
