//! Seeded scenes for layout and design renders. Not protocol data. `fixture` mirrors the worked
//! example in ARCHITECTURE.md §5.2; the others are TinyEMU-shaped functions of increasing size.

use crate::model::{Claim, Evidence, Graph, Group, Kind, Lang, Node, Pin, Ty, Wire};

struct B {
    g: Graph,
    group: Option<usize>,
}

const E: Ty = Ty::Exec;
const U: Ty = Ty::U32;
const BO: Ty = Ty::Bool;
const P: Ty = Ty::Ptr;

impl B {
    fn new(crumbs: Vec<&'static str>) -> B {
        B { g: Graph { crumbs: crumbs.into_iter().map(String::from).collect(), nodes: vec![], wires: vec![], groups: vec![], selected_group: None }, group: None }
    }

    fn node(&mut self, kind: Kind, title: &str, lang: Lang, ins: &[(&str, Ty)], outs: &[(&str, Ty)]) -> usize {
        let pins = |ps: &[(&str, Ty)]| ps.iter().map(|&(l, ty)| Pin { label: l.into(), ty, spelled: None }).collect();
        let (mut ins, mut outs): (Vec<Pin>, Vec<Pin>) = (pins(ins), pins(outs));
        label_unnamed(kind, &mut ins, &mut outs);
        self.g.nodes.push(Node {
            kind, title: title.into(), lang, evidence: Evidence::None, claim: None,
            ins, outs, group: self.group, members: 0, effects: None, rejection: None,
        });
        self.g.nodes.len() - 1
    }

    fn w(&mut self, a: usize, ao: usize, b: usize, bi: usize) {
        self.g.wires.push(Wire { from: (a, ao), to: (b, bi), back: false });
    }

    fn begin_group(&mut self, title: &str, lang: Lang, evidence: Evidence, claim: Option<Claim>) -> usize {
        self.g.groups.push(Group { title: title.into(), lang, evidence, claim, effects: None, rejection: None });
        self.group = Some(self.g.groups.len() - 1);
        self.g.groups.len() - 1
    }

    fn end_group(&mut self) {
        self.group = None;
    }

    fn mark(&mut self, n: usize, evidence: Evidence, claim: Option<Claim>) {
        self.g.nodes[n].evidence = evidence;
        self.g.nodes[n].claim = claim;
    }

    /// A pure binary op node: inputs a, b → out.
    fn op(&mut self, title: &str, lang: Lang, a: (usize, usize), b: Option<(usize, usize)>, out_ty: Ty) -> usize {
        let ins: &[(&str, Ty)] = if b.is_some() { &[("a", U), ("b", U)] } else { &[("x", U)] };
        let n = self.node(Kind::Op, title, lang, ins, &[("", out_ty)]);
        self.w(a.0, a.1, n, 0);
        if let Some(b) = b {
            self.w(b.0, b.1, n, 1);
        }
        n
    }
}

/// Name the pins a scene left unnamed, the way protocol pins are named: `run`/`then` for exec,
/// `start`/`end` on group input/output, `result` from calls, `value` otherwise. Constants and
/// getters stay pills whose title names their one output.
fn label_unnamed(kind: Kind, ins: &mut [Pin], outs: &mut [Pin]) {
    if ins.is_empty() && outs.len() == 1 && kind != Kind::GroupIn {
        return;
    }
    for p in ins.iter_mut().filter(|p| p.label.is_empty()) {
        p.label = match (p.ty, kind) {
            (Ty::Exec, Kind::GroupOut) => "end",
            (Ty::Exec, _) => "run",
            (_, Kind::GroupOut) => "result",
            _ => "value",
        }
        .into();
    }
    for p in outs.iter_mut().filter(|p| p.label.is_empty()) {
        p.label = match (p.ty, kind) {
            (Ty::Exec, Kind::GroupIn) => "start",
            (Ty::Exec, _) => "then",
            (_, Kind::Call | Kind::Group) => "result",
            _ => "value",
        }
        .into();
    }
}

const fn claim(worker: u8, active: bool) -> Option<Claim> {
    Some(Claim { worker, active })
}

pub fn scene(name: &str) -> Graph {
    let mut g = match name {
        "branches" => branches(),
        "dataflow" => dataflow(),
        "devices" => devices(),
        "interp" => interp(),
        _ => fixture(),
    };
    code_headers(&mut g);
    g
}

/// Headers say what construct a node is, in C terms: `if (cond)` with the condition's source,
/// `switch (opcode)`, `for (…)`, `return`, `x = …`.
fn code_headers(g: &mut Graph) {
    let source = |g: &Graph, node: usize, pin: usize| {
        g.wires.iter().find(|w| w.to == (node, pin)).map(|w| g.nodes[w.from.0].title.clone())
    };
    for i in 0..g.nodes.len() {
        let n = &g.nodes[i];
        let title = match n.kind {
            Kind::Branch if n.title == "Branch" => {
                let cond = n.ins.iter().position(|p| p.ty == Ty::Bool);
                match cond.and_then(|c| source(g, i, c)) {
                    Some(c) => format!("if ({c})"),
                    None => "if".into(),
                }
            }
            Kind::Switch => match n.title.strip_prefix("Switch ") {
                Some(sel) => format!("switch ({sel})"),
                None => continue,
            },
            Kind::GroupOut if n.title == "Return" => "return".into(),
            Kind::Set => match n.title.strip_prefix("Set ") {
                Some(var) => format!("{var} = …"),
                None => continue,
            },
            Kind::Loop if n.title == "for i in 0..n" => "for (i = 0; i < n; i++)".into(),
            _ => continue,
        };
        g.nodes[i].title = title;
    }
}

/// ARCHITECTURE.md §5.2: `compute(n)` with `step ∘ normalize` merged and expanded.
fn fixture() -> Graph {
    use Kind::*;
    let mut b = B::new(vec!["fixture", "compute"]);
    let input = b.node(GroupIn, "Input", Lang::None, &[], &[("", E), ("n", U)]);
    let acc0 = b.node(Local, "acc = 0", Lang::None, &[], &[("acc", U)]);
    let lp = b.node(Loop, "for i in 0..n", Lang::C, &[("", E), ("n", U), ("acc", U)],
        &[("loop body", E), ("index", U), ("acc", U), ("completed", E), ("final acc", U)]);
    let g = b.begin_group("step ∘ normalize", Lang::Rust, Evidence::Accepted, claim(2, false));
    let add = b.node(Op, "acc + i", Lang::Rust, &[("acc", U), ("i", U)], &[("sum", U)]);
    let norm = b.node(Call, "normalize", Lang::Rust, &[("x", U)], &[("result", U)]);
    b.end_group();
    let set = b.node(Set, "Set acc", Lang::None, &[("", E), ("value", U)], &[("", E), ("acc", U)]);
    let report = b.node(Call, "report", Lang::C, &[("", E), ("acc", U)], &[("", E)]);
    b.mark(report, Evidence::Rejected, claim(1, true));
    let out = b.node(GroupOut, "Output", Lang::None, &[("", E), ("acc", U)], &[]);
    b.w(input, 0, lp, 0);
    b.w(input, 1, lp, 1);
    b.w(acc0, 0, lp, 2);
    b.w(lp, 0, set, 0);
    b.w(lp, 2, add, 0);
    b.w(lp, 1, add, 1);
    b.w(add, 0, norm, 0);
    b.w(norm, 0, set, 1);
    b.w(set, 0, report, 0);
    b.w(set, 1, report, 1);
    b.w(lp, 3, out, 0);
    b.w(lp, 4, out, 1);
    b.g.selected_group = Some(g);
    b.g
}

/// `parse_header(buf, len)`: guard branches, a loop with a nested branch, early returns.
fn branches() -> Graph {
    use Kind::*;
    let c = Lang::C;
    let mut b = B::new(vec!["virtio", "parse_header"]);
    let input = b.node(GroupIn, "Input", Lang::None, &[], &[("", E), ("buf", P), ("len", U)]);
    let k8 = b.node(Local, "8", Lang::None, &[], &[("", U)]);
    let lt = b.op("len < 8", c, (input, 2), Some((k8, 0)), BO);
    let br1 = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(input, 0, br1, 0);
    b.w(lt, 0, br1, 1);
    let err1 = b.node(GroupOut, "Return", Lang::None, &[("", E), ("status", U)], &[]);
    let einval = b.node(Local, "-EINVAL", Lang::None, &[], &[("", U)]);
    b.w(br1, 0, err1, 0);
    b.w(einval, 0, err1, 1);
    let magic = b.node(Call, "read_u32", c, &[("buf", P), ("off", U)], &[("value", U)]);
    let k0 = b.node(Local, "0", Lang::None, &[], &[("", U)]);
    b.w(input, 1, magic, 0);
    b.w(k0, 0, magic, 1);
    let kmagic = b.node(Local, "MAGIC", Lang::None, &[], &[("", U)]);
    let eq = b.op("==", c, (magic, 0), Some((kmagic, 0)), BO);
    let br2 = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(br1, 1, br2, 0);
    b.w(eq, 0, br2, 1);
    let err2 = b.node(GroupOut, "Return", Lang::None, &[("", E), ("status", U)], &[]);
    let ebad = b.node(Local, "-EPROTO", Lang::None, &[], &[("", U)]);
    b.w(br2, 1, err2, 0);
    b.w(ebad, 0, err2, 1);
    let ver = b.node(Call, "read_u16", Lang::Rust, &[("buf", P), ("off", U)], &[("value", U)]);
    b.mark(ver, Evidence::Accepted, None);
    let k4 = b.node(Local, "4", Lang::None, &[], &[("", U)]);
    b.w(input, 1, ver, 0);
    b.w(k4, 0, ver, 1);
    let setv = b.node(Set, "Set version", Lang::None, &[("", E), ("value", U)], &[("", E), ("version", U)]);
    b.w(br2, 0, setv, 0);
    b.w(ver, 0, setv, 1);
    let count = b.node(Call, "entry_count", c, &[("buf", P), ("version", U)], &[("n", U)]);
    b.w(input, 1, count, 0);
    b.w(setv, 1, count, 1);
    let lp = b.node(Loop, "for i in 0..n", c, &[("", E), ("n", U), ("total", U)],
        &[("loop body", E), ("index", U), ("total", U), ("completed", E), ("final total", U)]);
    let t0 = b.node(Local, "total = 0", Lang::None, &[], &[("total", U)]);
    b.w(setv, 0, lp, 0);
    b.w(count, 0, lp, 1);
    b.w(t0, 0, lp, 2);
    let pe = b.node(Call, "parse_entry", c, &[("", E), ("buf", P), ("i", U)], &[("", E), ("ok", BO), ("size", U)]);
    b.mark(pe, Evidence::Rejected, claim(1, true));
    b.w(lp, 0, pe, 0);
    b.w(input, 1, pe, 1);
    b.w(lp, 1, pe, 2);
    let br3 = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(pe, 0, br3, 0);
    b.w(pe, 1, br3, 1);
    let add = b.op("total + size", c, (lp, 2), Some((pe, 2)), U);
    let sett = b.node(Set, "Set total", Lang::None, &[("", E), ("value", U)], &[("", E)]);
    b.w(br3, 0, sett, 0);
    b.w(add, 0, sett, 1);
    let warn = b.node(Call, "log_warn", c, &[("", E), ("i", U)], &[("", E)]);
    b.w(br3, 1, warn, 0);
    b.w(lp, 1, warn, 1);
    let ok = b.node(GroupOut, "Return", Lang::None, &[("", E), ("status", U)], &[]);
    b.w(lp, 3, ok, 0);
    b.w(lp, 4, ok, 1);
    b.g
}

/// Softfloat `mul64`: a mostly pure unpack → multiply → normalise → round → pack pipeline.
fn dataflow() -> Graph {
    use Kind::*;
    let r = Lang::Rust;
    let c = Lang::C;
    let mut b = B::new(vec!["softfp", "mul_sf64"]);
    let input = b.node(GroupIn, "Input", Lang::None, &[], &[("", E), ("a", U), ("b", U), ("rm", U)]);
    let mut parts = vec![];
    for (i, side) in ["a", "b"].iter().enumerate() {
        let src = (input, 1 + i);
        let sign = b.op(&format!("sign({side})"), r, src, None, BO);
        let exp = b.op(&format!("exp({side})"), r, src, None, U);
        let man = b.op(&format!("mant({side})"), r, src, None, U);
        b.mark(man, Evidence::Accepted, None);
        parts.push((sign, exp, man));
    }
    let g = b.begin_group("mul_core", r, Evidence::Accepted, claim(2, false));
    let xs = b.op("sign ^", r, (parts[0].0, 0), Some((parts[1].0, 0)), BO);
    let es = b.op("exp +", r, (parts[0].1, 0), Some((parts[1].1, 0)), U);
    let bias = b.op("- bias", r, (es, 0), None, U);
    let mm = b.op("mul_u64", r, (parts[0].2, 0), Some((parts[1].2, 0)), U);
    let hi = b.op("hi64", r, (mm, 0), None, U);
    b.end_group();
    let _ = g;
    let isnan = b.op("is_nan(a|b)", c, (parts[0].1, 0), Some((parts[1].1, 0)), BO);
    let br = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(input, 0, br, 0);
    b.w(isnan, 0, br, 1);
    let qnan = b.node(Call, "default_nan", c, &[("", E)], &[("", E), ("value", U)]);
    b.w(br, 0, qnan, 0);
    let ret1 = b.node(GroupOut, "Return", Lang::None, &[("", E), ("value", U)], &[]);
    b.w(qnan, 0, ret1, 0);
    b.w(qnan, 1, ret1, 1);
    let clz = b.op("clz", c, (hi, 0), None, U);
    let shl = b.op("<<", c, (hi, 0), Some((clz, 0)), U);
    let eadj = b.op("exp -", c, (bias, 0), Some((clz, 0)), U);
    let norm = b.node(Call, "normalize", c, &[("", E), ("mant", U), ("exp", U)], &[("", E), ("mant", U), ("exp", U)]);
    b.mark(norm, Evidence::None, claim(1, true));
    b.w(br, 1, norm, 0);
    b.w(shl, 0, norm, 1);
    b.w(eadj, 0, norm, 2);
    let round = b.node(Call, "round_pack", c, &[("", E), ("sign", BO), ("exp", U), ("mant", U), ("rm", U)], &[("", E), ("value", U), ("flags", U)]);
    b.w(norm, 0, round, 0);
    b.w(xs, 0, round, 1);
    b.w(norm, 2, round, 2);
    b.w(norm, 1, round, 3);
    b.w(input, 3, round, 4);
    let setf = b.node(Set, "Set fflags", Lang::None, &[("", E), ("value", U)], &[("", E)]);
    b.w(round, 0, setf, 0);
    b.w(round, 2, setf, 1);
    let ret = b.node(GroupOut, "Return", Lang::None, &[("", E), ("value", U)], &[]);
    b.w(setf, 0, ret, 0);
    b.w(round, 1, ret, 1);
    b.g
}

/// `virt_machine_init`: a long exec chain of device inits sharing state and config.
fn devices() -> Graph {
    use Kind::*;
    let c = Lang::C;
    let mut b = B::new(vec!["machine", "virt_machine_init"]);
    let input = b.node(GroupIn, "Input", Lang::None, &[], &[("", E), ("cfg", P), ("s", P)]);
    let devs = ["clint", "plic", "htif", "virtio_console", "virtio_net", "virtio_block", "virtio_9p", "virtio_input_kbd", "virtio_input_mouse", "simplefb", "rtc", "uart"];
    let mut prev = (input, 0);
    for (i, d) in devs.iter().enumerate() {
        let base = b.node(Call, "cfg_base", c, &[("cfg", P), ("dev", U)], &[("addr", U)]);
        b.w(input, 1, base, 0);
        let k = b.node(Local, &format!("{i}"), Lang::None, &[], &[("", U)]);
        b.w(k, 0, base, 1);
        let lang = if i % 3 == 0 { Lang::Rust } else { c };
        let n = b.node(Call, &format!("{d}_init"), lang, &[("", E), ("s", P), ("addr", U)], &[("", E), ("dev", P)]);
        if i % 3 == 0 {
            b.mark(n, Evidence::Accepted, None);
        }
        if i == 4 {
            b.mark(n, Evidence::Rejected, claim(1, true));
        }
        if i == 7 {
            b.mark(n, Evidence::None, claim(3, true));
        }
        b.w(prev.0, prev.1, n, 0);
        b.w(input, 2, n, 1);
        b.w(base, 0, n, 2);
        prev = (n, 0);
        if i % 4 == 3 {
            let reg = b.node(Call, "register_irq", c, &[("", E), ("s", P), ("dev", P)], &[("", E)]);
            b.w(prev.0, prev.1, reg, 0);
            b.w(input, 2, reg, 1);
            b.w(n, 1, reg, 2);
            prev = (reg, 0);
        }
    }
    let ret = b.node(GroupOut, "Return", Lang::None, &[("", E)], &[]);
    b.w(prev.0, prev.1, ret, 0);
    b.g
}

/// TinyEMU-shaped `execute_insn`: decode, a switch with many cases, shared tail, a merged ALU.
fn interp() -> Graph {
    use Kind::*;
    let c = Lang::C;
    let r = Lang::Rust;
    let mut b = B::new(vec!["riscv_cpu", "execute_insn"]);
    let input = b.node(GroupIn, "Input", Lang::None, &[], &[("", E), ("s", P), ("insn", U)]);
    let field = |b: &mut B, t: &str| b.op(t, r, (input, 2), None, U);
    let opcode = field(&mut b, "opcode");
    let rd = field(&mut b, "rd");
    let rs1 = field(&mut b, "rs1");
    let rs2 = field(&mut b, "rs2");
    let f3 = field(&mut b, "funct3");
    let imm_i = field(&mut b, "imm_i");
    let imm_s = field(&mut b, "imm_s");
    let imm_b = field(&mut b, "imm_b");
    for n in [opcode, rd, rs1, rs2, f3] {
        b.mark(n, Evidence::Accepted, None);
    }
    let sw = b.node(Switch, "Switch opcode", c, &[("", E), ("value", U)],
        &[("OP_IMM", E), ("OP", E), ("LOAD", E), ("STORE", E), ("BRANCH", E), ("JAL", E), ("default", E)]);
    b.w(input, 0, sw, 0);
    b.w(opcode, 0, sw, 1);
    let tail = b.node(Call, "pc += 4", c, &[("", E), ("s", P)], &[("", E)]);
    b.w(input, 1, tail, 1);
    let ret = b.node(GroupOut, "Return", Lang::None, &[("", E)], &[]);
    b.w(tail, 0, ret, 0);
    let read = |b: &mut B, reg: usize| {
        let n = b.node(Call, "read_reg", r, &[("s", P), ("r", U)], &[("value", U)]);
        b.w(input, 1, n, 0);
        b.w(reg, 0, n, 1);
        n
    };
    let write = |b: &mut B, from: (usize, usize), val: (usize, usize)| {
        let n = b.node(Call, "write_reg", c, &[("", E), ("s", P), ("rd", U), ("value", U)], &[("", E)]);
        b.w(from.0, from.1, n, 0);
        b.w(input, 1, n, 1);
        b.w(rd, 0, n, 2);
        b.w(val.0, val.1, n, 3);
        n
    };
    // OP_IMM
    let a = read(&mut b, rs1);
    let addi = b.op("a + imm", c, (a, 0), Some((imm_i, 0)), U);
    let w1 = write(&mut b, (sw, 0), (addi, 0));
    b.w(w1, 0, tail, 0);
    // OP: merged Rust ALU, expanded.
    let a2 = read(&mut b, rs1);
    let b2 = read(&mut b, rs2);
    b.begin_group("alu", r, Evidence::Accepted, claim(2, false));
    let add = b.op("add", r, (a2, 0), Some((b2, 0)), U);
    let sub = b.op("sub", r, (a2, 0), Some((b2, 0)), U);
    let xor = b.op("xor", r, (a2, 0), Some((b2, 0)), U);
    let sll = b.op("sll", r, (a2, 0), Some((b2, 0)), U);
    let sel = b.node(Op, "select funct3", r, &[("funct3", U), ("add", U), ("sub", U), ("xor", U), ("sll", U)], &[("", U)]);
    b.w(f3, 0, sel, 0);
    b.w(add, 0, sel, 1);
    b.w(sub, 0, sel, 2);
    b.w(xor, 0, sel, 3);
    b.w(sll, 0, sel, 4);
    b.end_group();
    let w2 = write(&mut b, (sw, 1), (sel, 0));
    b.w(w2, 0, tail, 0);
    // LOAD
    let a3 = read(&mut b, rs1);
    let addr = b.op("a + imm", c, (a3, 0), Some((imm_i, 0)), U);
    let ld = b.node(Call, "mem_read", c, &[("", E), ("s", P), ("addr", U)], &[("", E), ("fault", BO), ("value", U)]);
    b.mark(ld, Evidence::Rejected, claim(1, true));
    b.w(sw, 2, ld, 0);
    b.w(input, 1, ld, 1);
    b.w(addr, 0, ld, 2);
    let brl = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(ld, 0, brl, 0);
    b.w(ld, 1, brl, 1);
    let exc = b.node(Call, "raise_exception", c, &[("", E), ("s", P), ("cause", U)], &[]);
    let kc = b.node(Local, "LOAD_FAULT", Lang::None, &[], &[("", U)]);
    b.w(brl, 0, exc, 0);
    b.w(input, 1, exc, 1);
    b.w(kc, 0, exc, 2);
    let w3 = write(&mut b, (brl, 1), (ld, 2));
    b.w(w3, 0, tail, 0);
    // STORE
    let a4 = read(&mut b, rs1);
    let v4 = read(&mut b, rs2);
    let saddr = b.op("a + imm", c, (a4, 0), Some((imm_s, 0)), U);
    let st = b.node(Call, "mem_write", c, &[("", E), ("s", P), ("addr", U), ("value", U)], &[("", E), ("fault", BO)]);
    b.w(sw, 3, st, 0);
    b.w(input, 1, st, 1);
    b.w(saddr, 0, st, 2);
    b.w(v4, 0, st, 3);
    let brs = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(st, 0, brs, 0);
    b.w(st, 1, brs, 1);
    let exc2 = b.node(Call, "raise_exception", c, &[("", E), ("s", P), ("cause", U)], &[]);
    let kc2 = b.node(Local, "STORE_FAULT", Lang::None, &[], &[("", U)]);
    b.w(brs, 0, exc2, 0);
    b.w(input, 1, exc2, 1);
    b.w(kc2, 0, exc2, 2);
    b.w(brs, 1, tail, 0);
    // BRANCH
    let a5 = read(&mut b, rs1);
    let b5 = read(&mut b, rs2);
    let cmp = b.node(Op, "compare funct3", c, &[("funct3", U), ("a", U), ("b", U)], &[("taken", BO)]);
    b.w(f3, 0, cmp, 0);
    b.w(a5, 0, cmp, 1);
    b.w(b5, 0, cmp, 2);
    let brb = b.node(Branch, "Branch", c, &[("", E), ("cond", BO)], &[("true", E), ("false", E)]);
    b.w(sw, 4, brb, 0);
    b.w(cmp, 0, brb, 1);
    let jmp = b.node(Call, "pc += imm", c, &[("", E), ("s", P), ("imm", U)], &[("", E)]);
    b.w(brb, 0, jmp, 0);
    b.w(input, 1, jmp, 1);
    b.w(imm_b, 0, jmp, 2);
    b.w(jmp, 0, ret, 0);
    b.w(brb, 1, tail, 0);
    // JAL
    let link = b.node(Call, "pc + 4", r, &[("s", P)], &[("", U)]);
    b.w(input, 1, link, 0);
    let w6 = write(&mut b, (sw, 5), (link, 0));
    let jal = b.node(Call, "pc += imm", c, &[("", E), ("s", P), ("imm", U)], &[("", E)]);
    b.w(w6, 0, jal, 0);
    b.w(input, 1, jal, 1);
    b.w(imm_b, 0, jal, 2);
    b.w(jal, 0, ret, 0);
    // default
    let ill = b.node(Call, "raise_exception", c, &[("", E), ("s", P), ("cause", U)], &[]);
    let kill = b.node(Local, "ILLEGAL_INSN", Lang::None, &[], &[("", U)]);
    b.w(sw, 6, ill, 0);
    b.w(input, 1, ill, 1);
    b.w(kill, 0, ill, 2);
    b.g
}


#[cfg(test)]
mod tests {
    use crate::model::Graph;

    /// Every pin outside a titled pill has a label, and no node repeats one on a side.
    pub(crate) fn problems(name: &str, g: &Graph) -> Vec<String> {
        let mut problems = Vec::new();
        for n in &g.nodes {
            let pill = n.ins.is_empty() && n.outs.len() == 1 && n.outs[0].label.is_empty();
            if pill {
                continue;
            }
            for (side, pins) in [("in", &n.ins), ("out", &n.outs)] {
                let mut seen = std::collections::HashSet::new();
                for p in pins.iter() {
                    if p.label.is_empty() {
                        problems.push(format!("{name}: {} has an unlabelled {side} pin", n.title));
                    } else if !seen.insert(&p.label) {
                        problems.push(format!("{name}: {} repeats {side} label {:?}", n.title, p.label));
                    }
                }
            }
        }
        problems.dedup();
        problems
    }

    #[test]
    fn every_scene_pin_is_labelled() {
        let all: Vec<_> = ["fixture", "branches", "dataflow", "devices", "interp"]
            .into_iter()
            .flat_map(|name| problems(name, &crate::layout::with_getters(super::scene(name))))
            .collect();
        assert!(all.is_empty(), "{}", all.join("\n"));
    }

    #[test]
    fn protocol_projection_is_labelled() {
        let (_, ids) = argo_protocol::fixture::graph();
        let open = [ids.loop_region, ids.loop_body].into_iter().collect();
        let p = crate::bridge::project(&argo_protocol::fixture::snapshot(), Some(ids.compute), &open);
        let found = problems("protocol", &p.graph);
        assert!(found.is_empty(), "{}", found.join("\n"));
    }
}
