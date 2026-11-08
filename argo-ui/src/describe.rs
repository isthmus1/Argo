//! Hover-card content derived from a node's pins, connections and kind: a signature line,
//! its connections, inferred effects and migration status. View text only; never semantic truth.

use crate::model::{Graph, Kind, Lang, Node, Pin, Ty};

pub struct Card {
    pub signature: String,
    /// (pin, type, type spelled in the node's language, connected to) for each data input.
    pub inputs: Vec<(String, Ty, String, String)>,
    /// (pin, type, type spelled in the node's language, consumers) for each data output.
    pub outputs: Vec<(String, Ty, String, String)>,
    /// Exec flow: where control arrives from and goes to.
    pub flow: Option<String>,
    pub effects: Vec<&'static str>,
}

fn c_ty(ty: Ty) -> &'static str {
    match ty {
        Ty::Exec => "void",
        Ty::U32 => "uint32_t",
        Ty::Bool => "bool",
        Ty::Ptr => "void *",
    }
}

/// A pin's C spelling: exact when the source knows it, else derived from its view type.
fn c_of(p: &Pin) -> String {
    p.spelled.as_ref().map_or_else(|| c_ty(p.ty).to_string(), |[c, _]| c.clone())
}

/// A pin's Rust spelling at the current ABI.
fn rs_of(p: &Pin) -> String {
    p.spelled.as_ref().map_or_else(|| ty_name(p.ty).to_string(), |[_, rs]| rs.clone())
}

/// Type text in the node's implementation language (C nodes read as C).
fn pin_ty(lang: Lang, p: &Pin) -> String {
    if lang == Lang::C { c_of(p) } else { rs_of(p) }
}

/// C prototype: the first data output is the return value, the rest become out-parameters.
fn c_prototype(n: &Node) -> String {
    let outs: Vec<_> = n.outs.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec).collect();
    let ret = outs.first().map_or_else(|| "void".to_string(), |(_, p)| c_of(p));
    let mut ps: Vec<String> = n.ins.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec)
        .map(|(i, p)| { let t = c_of(p); let sep = if t.ends_with('*') { "" } else { " " }; format!("{t}{sep}{}", pin_name(&p.label, i)) })
        .collect();
    ps.extend(outs.iter().skip(1).map(|(i, p)| format!("{} *{}", c_of(p), pin_name(&p.label, *i))));
    let ps = if ps.is_empty() { "void".to_string() } else { ps.join(", ") };
    format!("{ret} {}({ps});", ident(&n.title))
}

pub fn ty_name(ty: Ty) -> &'static str {
    match ty {
        Ty::Exec => "exec",
        Ty::U32 => "u32",
        Ty::Bool => "bool",
        Ty::Ptr => "*mut T",
    }
}

fn pin_name(label: &str, i: usize) -> String {
    match (label.is_empty(), i) {
        (true, 0) => "out".into(),
        (true, _) => format!("out{i}"),
        _ => label.replace(' ', "_"),
    }
}

fn params(n: &Node) -> String {
    n.ins.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec)
        .map(|(i, p)| format!("{}: {}", pin_name(&p.label, i), rs_of(p)))
        .collect::<Vec<_>>().join(", ")
}

fn returns(n: &Node) -> String {
    let outs: Vec<_> = n.outs.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec).collect();
    match outs.as_slice() {
        [] => String::new(),
        [(_, p)] => format!(" -> {}", rs_of(p)),
        many => format!(" -> ({})", many.iter().map(|(i, p)| format!("{}: {}", pin_name(&p.label, *i), rs_of(p))).collect::<Vec<_>>().join(", ")),
    }
}

fn ident(title: &str) -> String {
    title.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn signature(g: &Graph, n: &Node) -> String {
    match n.kind {
        Kind::Call | Kind::Group if n.lang == Lang::C => c_prototype(n),
        Kind::Call | Kind::Group => format!("fn {}({}){}", ident(&n.title), params(n), returns(n)),
        Kind::Op => format!("{}  ({}){}", n.title, params(n), returns(n)),
        Kind::Loop => n.title.clone(),
        Kind::Branch => "if cond { true } else { false }".into(),
        Kind::Switch => {
            let arms: Vec<_> = n.outs.iter().filter(|p| p.ty == Ty::Exec).map(|p| p.label.as_str()).collect();
            format!("switch value {{ {} }}", arms.join(", "))
        }
        Kind::Set => format!("{} = value", n.title.trim_end_matches(" = …")),
        Kind::Local => {
            let ty = n.outs.first().map_or_else(|| "u32".to_string(), rs_of);
            match n.title.split_once(" = ") {
                Some((name, v)) => format!("let {name}: {ty} = {v}"),
                None => format!("const {}: {ty}", n.title),
            }
        }
        Kind::Get => format!("param {}: {}", n.title, n.outs.first().map_or_else(|| "u32".to_string(), rs_of)),
        Kind::GroupIn => {
            let ps: Vec<_> = n.outs.iter().filter(|p| p.ty != Ty::Exec).map(|p| format!("{}: {}", p.label, rs_of(p))).collect();
            format!("fn {}({})", g.crumbs.last().map_or("?", String::as_str), ps.join(", "))
        }
        Kind::GroupOut => {
            let rs: Vec<_> = n.ins.iter().filter(|p| p.ty != Ty::Exec).map(|p| format!("{}: {}", p.label, rs_of(p))).collect();
            if rs.is_empty() { "return".into() } else { format!("return {}", rs.join(", ")) }
        }
        Kind::Region | Kind::Case => n.title.clone(),
        Kind::Type => format!("type {}", n.title),
    }
}

/// A node's name, disambiguated when several nodes share it: `read_reg(rs1)` names the
/// source of its last data input.
fn name(g: &Graph, node: usize) -> String {
    let t = &g.nodes[node].title;
    if g.nodes.iter().filter(|n| &n.title == t).count() < 2 {
        return t.clone();
    }
    let arg = g.wires.iter().filter(|w| w.to.0 == node && g.nodes[node].ins[w.to.1].ty != Ty::Exec).max_by_key(|w| w.to.1)
        .map(|w| g.nodes[w.from.0].title.clone());
    match arg {
        Some(a) => format!("{t}({a})"),
        None => t.clone(),
    }
}

fn endpoint(g: &Graph, node: usize, label: &str) -> String {
    let t = name(g, node);
    let single_out = g.nodes[node].outs.iter().filter(|p| p.ty != Ty::Exec).count() == 1;
    if label.is_empty() || single_out { t } else { format!("{t}.{label}") }
}

pub fn card(g: &Graph, i: usize) -> Card {
    let n = &g.nodes[i];
    let mut inputs = vec![];
    for (pi, p) in n.ins.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec) {
        let src = g.wires.iter().find(|w| w.to == (i, pi))
            .map(|w| endpoint(g, w.from.0, &g.nodes[w.from.0].outs[w.from.1].label))
            .unwrap_or_else(|| "unconnected".into());
        inputs.push((pin_name(&p.label, pi), p.ty, pin_ty(n.lang, p), src));
    }
    let mut outputs = vec![];
    for (k, (pi, p)) in n.outs.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec).enumerate() {
        let uses: Vec<_> = g.wires.iter().filter(|w| w.from == (i, pi)).map(|w| name(g, w.to.0)).collect();
        let to = match uses.len() {
            0 => "unused".into(),
            1 => uses[0].clone(),
            k => format!("{} +{} more", uses[0], k - 1),
        };
        outputs.push((pin_name(&p.label, k), p.ty, pin_ty(n.lang, p), to));
    }
    let from: Vec<_> = g.wires.iter().filter(|w| w.to.0 == i && n.ins[w.to.1].ty == Ty::Exec).map(|w| g.nodes[w.from.0].title.clone()).collect();
    let to: Vec<_> = g.wires.iter().filter(|w| w.from.0 == i && n.outs[w.from.1].ty == Ty::Exec).map(|w| g.nodes[w.to.0].title.clone()).collect();
    let flow = (!from.is_empty() || !to.is_empty()).then(|| {
        let a = if from.is_empty() { "entry".to_string() } else { from.join(", ") };
        let b = if to.is_empty() { "exit".to_string() } else if to.len() > 3 { format!("{} arms", to.len()) } else { to.join(", ") };
        format!("{a} → {b}")
    });

    let has_exec = n.ins.iter().chain(&n.outs).any(|p| p.ty == Ty::Exec);
    let mut effects = vec![];
    match n.kind {
        Kind::Branch | Kind::Switch | Kind::Loop => effects.push("control flow"),
        Kind::Set => effects.push("writes local"),
        Kind::GroupOut => effects.push("returns"),
        _ => {}
    }
    if n.ins.iter().any(|p| p.ty == Ty::Ptr) {
        effects.push(if has_exec { "memory read/write" } else { "memory read" });
    }
    if matches!(n.kind, Kind::Call | Kind::Group) && has_exec && effects.is_empty() {
        effects.push("side effects");
    }
    if !has_exec && effects.is_empty() {
        effects.push("pure");
    }

    Card { signature: signature(g, n), inputs, outputs, flow, effects }
}
