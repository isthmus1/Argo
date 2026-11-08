//! Hover-card content derived from a node's pins, connections and kind: a signature line,
//! its connections, effects (declared by the protocol, else inferred from shape and marked so)
//! and migration status. View text only; never semantic truth.

use crate::model::{Effect, Graph, Kind, Lang, Node, Pin, Ty};

/// One data pin's row: its name and type, and what it connects to.
pub struct Port {
    pub name: String,
    pub ty: Ty,
    /// The type spelled in the node's language.
    pub ty_text: String,
    /// The other end(s) as compact `node.pin` names.
    pub other: String,
    /// The same with full node titles, for a tooltip.
    pub full: String,
}

/// The effect badge: filled dot (has effects), hollow dot (pure), ring with a `?` (unknown, or
/// only inferred from the node's shape).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Effects,
    Pure,
    Unknown,
}

pub struct Card {
    pub signature: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// Exec flow: where control arrives from and goes to, by short names; and by full titles.
    pub flow: Option<(String, String)>,
    pub effects: Vec<&'static str>,
    /// Whether `effects` were guessed from the node's shape rather than declared.
    pub inferred: bool,
    pub mark: Mark,
}

impl Card {
    /// Whether the header shows the effect badge. Control flow alone is already named by the
    /// kicker, so a guessed "control flow" adds nothing.
    pub fn badge(&self) -> bool {
        !self.inferred || self.effects.iter().any(|e| *e != "control flow")
    }

    /// The badge's tooltip.
    pub fn effect_tip(&self) -> String {
        let list = self.effects.join(", ");
        if self.inferred { format!("Effects not analysed · inferred: {list}") } else { format!("Effects: {list}") }
    }
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
    format!("{ret} {}({ps});", fn_name(n))
}

/// A function-like node's name as an identifier: a call site by its callee (`step(acc, i)` →
/// `step`), a group by its title (`step ∘ normalize` → `step_normalize`).
fn fn_name(n: &Node) -> String {
    ident(n.title.split_once('(').map_or(n.title.as_str(), |(callee, _)| callee))
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

/// A title as an identifier: each run of other characters becomes one `_`, none at the ends
/// (`step ∘ normalize` → `step_normalize`).
fn ident(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in title.chars() {
        if c.is_alphanumeric() || c == '_' {
            if gap && !out.is_empty() {
                out.push('_');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

fn signature(g: &Graph, n: &Node) -> String {
    match n.kind {
        Kind::Call | Kind::Group if n.lang == Lang::C => c_prototype(n),
        Kind::Call | Kind::Group => format!("fn {}({}){}", fn_name(n), params(n), returns(n)),
        Kind::Op => format!("{}  ({}){}", n.title, params(n), returns(n)),
        Kind::Loop => n.title.clone(),
        Kind::Branch => "if cond { true } else { false }".into(),
        Kind::Switch => {
            let arms: Vec<_> = n.outs.iter().filter(|p| p.ty == Ty::Exec).map(|p| p.label.as_str()).collect();
            format!("switch value {{ {} }}", arms.join(", "))
        }
        Kind::Set => format!("{} = value", set_var(&n.title)),
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

/// The variable an assignment's title names: `acc =` (or an older `acc = …`) → `acc`.
fn set_var(title: &str) -> &str {
    title.trim_end_matches('…').trim_end().trim_end_matches('=').trim_end()
}

/// Titles up to this many characters name a node as they are; longer ones are shortened.
const SHORT: usize = 16;

/// A node's compact name for endpoint text: control flow and regions by a kind word (`loop`,
/// `if`), calls by their callee (`step`), declarations by their variable (`acc`), any other
/// long title cut with an ellipsis.
fn short(n: &Node) -> String {
    let t = n.title.trim();
    match n.kind {
        Kind::Loop => return "loop".into(),
        Kind::Branch => return "if".into(),
        Kind::Switch => return "switch".into(),
        Kind::Case => return "case".into(),
        Kind::Region => return "block".into(),
        Kind::Call => {
            if let Some((callee, _)) = t.split_once('(') {
                return callee.trim().to_string();
            }
        }
        // A declaration by its variable: `uint32_t acc = 0` → `acc`.
        Kind::Local | Kind::Set => {
            if let Some((lhs, _)) = t.split_once('=')
                && let Some(var) = lhs.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).find(|w| !w.is_empty())
            {
                return var.to_string();
            }
        }
        _ => {}
    }
    if t.chars().count() <= SHORT {
        return t.to_string();
    }
    match n.kind {
        Kind::GroupIn => "input".into(),
        Kind::GroupOut => "output".into(),
        _ => format!("{}…", t.chars().take(SHORT - 1).collect::<String>().trim_end()),
    }
}

/// A node's short name, disambiguated when other nodes share it: `read_reg(rs1)` names the
/// source of its last data input.
fn name(g: &Graph, node: usize) -> String {
    let s = short(&g.nodes[node]);
    if g.nodes.iter().filter(|n| short(n) == s).count() < 2 {
        return s;
    }
    let arg = g.wires.iter().filter(|w| w.to.0 == node && g.nodes[node].ins[w.to.1].ty != Ty::Exec).max_by_key(|w| w.to.1)
        .map(|w| short(&g.nodes[w.from.0]));
    match arg {
        Some(a) => format!("{s}({a})"),
        None => s,
    }
}

/// One end of a wire as `node.pin` (the pin label kept, so several wires to one node stay
/// distinct), and the same with the node's full title.
fn endpoint(g: &Graph, node: usize, label: &str) -> (String, String) {
    let full = g.nodes[node].title.trim().to_string();
    if label.trim().is_empty() {
        return (name(g, node), full);
    }
    let pin = label.trim().replace(' ', "_");
    (format!("{}.{pin}", name(g, node)), format!("{full} · {}", label.trim()))
}

/// Endpoint text (`node.pin`, maybe `+N more` after it) cut to at most `max` characters while
/// keeping the pin. A producer with a disambiguating `(arg)` loses its name and keeps the
/// argument (`read_reg(rs1).value` → `…(rs1).value`); one without, or whose argument doesn't
/// fit, keeps its start (`read_reg….value`). Text without a pin, or whose pin alone doesn't
/// fit, keeps its end (`…ng_pin_name`). Every cut is one `…`.
pub fn fit_end(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let take = |s: &str, k: usize| s.chars().take(k).collect::<String>();
    let tail = |s: &str, k: usize| {
        let c = s.chars().count();
        s.chars().skip(c.saturating_sub(k)).collect::<String>()
    };
    let head_cut = || format!("…{}", tail(text, max.saturating_sub(1)));
    // The pin: from the last `.` outside the producer's parentheses.
    let Some(dot) = text.rfind('.').filter(|&d| d > 0 && !text[d..].contains(')')) else { return head_cut() };
    let (producer, pin) = text.split_at(dot);
    let pin_n = pin.chars().count();
    if let Some(open) = producer.rfind('(').filter(|&o| o > 0 && producer.ends_with(')')) {
        let arg = &producer[open..];
        if 1 + arg.chars().count() + pin_n <= max {
            return format!("…{arg}{pin}");
        }
    }
    // Room for at least one producer character and the ellipsis.
    let Some(room) = max.checked_sub(pin_n + 1).filter(|r| *r >= 1) else { return head_cut() };
    format!("{}…{pin}", take(producer, room).trim_end())
}

/// The badge and labels for declared effects: unknown if any part is, pure only if all is.
pub fn declared(effects: &[Effect]) -> (Mark, Vec<&'static str>) {
    let mark = if effects.contains(&Effect::Unknown) {
        Mark::Unknown
    } else if effects.iter().all(|e| *e == Effect::Pure) {
        Mark::Pure
    } else {
        Mark::Effects
    };
    (mark, effects.iter().map(|e| e.label()).collect())
}

pub fn card(g: &Graph, i: usize) -> Card {
    let n = &g.nodes[i];
    let mut inputs = vec![];
    for (pi, p) in n.ins.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec) {
        let (other, full) = g.wires.iter().find(|w| w.to == (i, pi))
            .map(|w| endpoint(g, w.from.0, &g.nodes[w.from.0].outs[w.from.1].label))
            .unwrap_or_else(|| ("unconnected".into(), "unconnected".into()));
        inputs.push(Port { name: pin_name(&p.label, pi), ty: p.ty, ty_text: pin_ty(n.lang, p), other, full });
    }
    let mut outputs = vec![];
    for (k, (pi, p)) in n.outs.iter().enumerate().filter(|(_, p)| p.ty != Ty::Exec).enumerate() {
        let uses: Vec<_> = g.wires.iter().filter(|w| w.from == (i, pi)).map(|w| endpoint(g, w.to.0, &g.nodes[w.to.0].ins[w.to.1].label)).collect();
        let (other, full) = match uses.len() {
            0 => ("unused".into(), "unused".into()),
            1 => uses[0].clone(),
            k => (format!("{} +{} more", uses[0].0, k - 1), uses.iter().map(|u| u.1.as_str()).collect::<Vec<_>>().join("\n")),
        };
        outputs.push(Port { name: pin_name(&p.label, k), ty: p.ty, ty_text: pin_ty(n.lang, p), other, full });
    }
    let from: Vec<_> = g.wires.iter().filter(|w| w.to.0 == i && n.ins[w.to.1].ty == Ty::Exec).map(|w| w.from.0).collect();
    let to: Vec<_> = g.wires.iter().filter(|w| w.from.0 == i && n.outs[w.from.1].ty == Ty::Exec).map(|w| w.to.0).collect();
    let flow = (!from.is_empty() || !to.is_empty()).then(|| {
        let side = |ends: &[usize], none: &str, by: &dyn Fn(usize) -> String| {
            if ends.is_empty() { none.to_string() } else { ends.iter().map(|&e| by(e)).collect::<Vec<_>>().join(", ") }
        };
        let arms = |ends: &[usize], by: &dyn Fn(usize) -> String| if ends.len() > 3 { format!("{} arms", ends.len()) } else { side(ends, "exit", by) };
        let by_name = |e: usize| name(g, e);
        let by_title = |e: usize| g.nodes[e].title.trim().to_string();
        (format!("{} → {}", side(&from, "entry", &by_name), arms(&to, &by_name)),
         format!("{} → {}", side(&from, "entry", &by_title), side(&to, "exit", &by_title)))
    });

    if let Some(list) = &n.effects {
        let (mark, effects) = declared(list);
        return Card { signature: signature(g, n), inputs, outputs, flow, effects, inferred: false, mark };
    }

    // Not analysed: guess from the node's shape, and say so (the badge shows "unknown").
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
        effects.push("possible side effects");
    }
    if !has_exec && effects.is_empty() {
        effects.push("pure");
    }

    Card { signature: signature(g, n), inputs, outputs, flow, effects, inferred: true, mark: Mark::Unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::State;

    #[test]
    fn identifiers_collapse_runs_of_other_characters() {
        assert_eq!(ident("step ∘ normalize"), "step_normalize");
        assert_eq!(ident("  a + imm "), "a_imm");
        assert_eq!(ident("read_reg"), "read_reg");
        assert_eq!(ident("pc += 4"), "pc_4");
    }

    #[test]
    fn a_collapsed_groups_signature_is_one_identifier() {
        let mut s = State::load("fixture");
        let g = s.graph.groups.iter().position(|g| g.title == "step ∘ normalize").unwrap();
        s.collapse_group(g);
        let i = s.view.nodes.iter().position(|n| n.kind == Kind::Group).unwrap();
        let c = card(&s.view, i);
        assert!(c.signature.contains("fn step_normalize("), "{}", c.signature);
    }

    #[test]
    fn endpoints_to_one_loop_stay_distinct_and_short() {
        let s = State::protocol_fixture();
        let step = s.view.nodes.iter().position(|n| n.title == "step(acc, i)").unwrap();
        let c = card(&s.view, step);
        let ins: Vec<_> = c.inputs.iter().map(|p| p.other.as_str()).collect();
        assert_eq!(ins.len(), 2, "{ins:?}");
        assert!(ins.iter().all(|e| e.starts_with("loop.")), "{ins:?}");
        assert_ne!(ins[0], ins[1], "two pins of the same loop read the same");
        for p in &c.inputs {
            assert!(p.full.starts_with("for (uint32_t i = 0; i < n; i++) · "), "{}", p.full);
        }
        // And the other way: the loop's outputs name their consumers by node and pin.
        let lp = s.view.nodes.iter().position(|n| n.kind == Kind::Loop).unwrap();
        let outs: Vec<_> = card(&s.view, lp).outputs.into_iter().map(|p| p.other).collect();
        assert!(outs.iter().any(|o| o.starts_with("step.acc")) && outs.iter().any(|o| o == "step.i"), "{outs:?}");
    }

    #[test]
    fn declared_effects_win_over_shape() {
        let s = State::protocol_fixture();
        let step = s.view.nodes.iter().position(|n| n.title == "step(acc, i)").unwrap();
        let c = card(&s.view, step);
        assert_eq!((c.mark, c.inferred), (Mark::Pure, false));
        assert_eq!(c.effects, ["pure"]);
        assert!(c.signature.starts_with("uint32_t step(uint32_t acc"), "a call is named by its callee: {}", c.signature);
        // A mock call with exec pins is only guessed at: no effect claim.
        let m = State::load("fixture");
        let report = m.view.nodes.iter().position(|n| n.title == "report").unwrap();
        let c = card(&m.view, report);
        assert_eq!((c.mark, c.inferred), (Mark::Unknown, true));
        assert!(c.effect_tip().contains("inferred"));
    }

    #[test]
    fn cut_endpoints_keep_their_pin() {
        // Short enough: as is.
        assert_eq!(fit_end("acc.value", 16), "acc.value");
        assert_eq!(fit_end("unconnected", 16), "unconnected");
        // The producer is cut, keeping its disambiguating argument while it fits.
        // The producer's name goes; its disambiguating argument and the pin stay.
        assert_eq!(fit_end("read_reg(rs1).value", 16), "…(rs1).value");
        assert_eq!(fit_end("write_reg(alu).value", 19), "…(alu).value");
        assert_eq!(fit_end("read_reg(rs1).value", 19), "read_reg(rs1).value");
        assert_eq!(fit_end("acc(normalize).acc", 16), "…(normalize).acc");
        assert_eq!(fit_end("acc(step ∘ normalize).value", 16), "acc(step….value");
        assert_eq!(fit_end("x_really_long_name.result", 14), "x_real….result");
        // A "+N more" tail rides with the pin.
        assert_eq!(fit_end("x_really_long_name.acc +2 more", 20), "x_reall….acc +2 more");
        // No pin, or a pin too long to keep whole: the end survives.
        assert_eq!(fit_end("a_long_name_without_pin", 10), "…thout_pin");
        assert_eq!(fit_end("f.an_extremely_long_pin_name", 10), "…_pin_name");
        // A dot inside the producer's parentheses is not a pin.
        assert_eq!(fit_end("call(a.b, c, d, e)", 8), "…, d, e)");
        for t in ["read_reg(rs1).value", "acc(step ∘ normalize).value", "x_really_long_name.acc +2 more", "a_long_name"] {
            for max in 1..30 {
                assert!(fit_end(t, max).matches('…').count() <= 1, "{t} {max}: one ellipsis at most");
            }
        }
        for (t, max) in [("read_reg(rs1).value", 16), ("acc(normalize).acc", 12), ("a_long_name_without_pin", 10), ("x.y", 1)] {
            assert!(fit_end(t, max).chars().count() <= max.max(1), "{t} {max}");
        }
    }

    #[test]
    fn assignments_read_as_their_variable() {
        assert_eq!(set_var("acc ="), "acc");
        assert_eq!(set_var("acc = …"), "acc");
        let s = State::load("fixture");
        let set = s.view.nodes.iter().position(|n| n.kind == Kind::Set).unwrap();
        assert_eq!(s.view.nodes[set].title, "acc =");
        assert_eq!(card(&s.view, set).signature, "acc = value");
        assert_eq!(short(&s.view.nodes[set]), "acc");
    }

    #[test]
    fn short_names() {
        let node = |kind, title: &str| Node {
            kind, title: title.into(), lang: Lang::C, evidence: crate::model::Evidence::None, claim: None,
            outs: vec![], ins: vec![], group: None, members: 0, effects: None, rejection: None,
        };
        assert_eq!(short(&node(Kind::Loop, "for (uint32_t i = 0; i < n; i++)")), "loop");
        assert_eq!(short(&node(Kind::Call, "step(acc, i)")), "step");
        assert_eq!(short(&node(Kind::Local, "uint32_t acc = 0")), "acc");
        assert_eq!(short(&node(Kind::Op, "a + imm")), "a + imm");
        assert_eq!(short(&node(Kind::GroupOut, "Output · returns acc")), "output");
        assert_eq!(short(&node(Kind::Op, "x_really_long_expression")), "x_really_long_e…");
    }
}
