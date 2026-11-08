//! Who calls whom among a program's functions, for the whole-program view.
//!
//! The protocol graph records syntax only, with no call relations between functions, so this
//! reads each function's text in the snapshot's (preprocessed) sources and keeps every direct
//! call `f(...)` whose callee is a function of the program. Calls through pointers name no
//! function and are left out.
//!
//! The text is scanned, not parsed: a unit's recorded source holds only its own items, without
//! the typedefs it uses from system headers or from items another unit recorded, so it can't be
//! parsed on its own. In preprocessed C (no comments, no macros) a direct call is a name followed
//! by `(`, outside string and character literals and not after `.` or `->`.

use std::collections::{HashMap, HashSet};

use argo_protocol::{NodeId, NodeKind, Snapshot};

/// Caller and callee, each once per pair, in source order. No self-calls.
pub fn calls(snap: &Snapshot) -> Vec<(NodeId, NodeId)> {
    let functions: Vec<_> = snap.graph.nodes.iter().filter(|n| n.kind == NodeKind::Function).collect();
    let mut by_name: HashMap<&str, Vec<(NodeId, Option<NodeId>)>> = HashMap::new();
    for n in &functions {
        by_name.entry(n.name.as_str()).or_default().push((n.id, n.parent));
    }
    let module: HashMap<NodeId, Option<NodeId>> = functions.iter().map(|n| (n.id, n.parent)).collect();
    // Each function's text, in source order: by file as the snapshot lists them, then by offset.
    let order: HashMap<_, usize> = snap.sources.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    let mut bodies: Vec<(usize, u32, NodeId, &str)> = functions
        .iter()
        .filter_map(|n| {
            let r = n.provenance.as_ref()?.range;
            let file = *order.get(&r.file)?;
            let text = snap.sources[file].text.get(r.lo as usize..r.hi as usize)?;
            Some((file, r.lo, n.id, text))
        })
        .collect();
    bodies.sort_by_key(|&(file, lo, ..)| (file, lo));

    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for (_, _, caller, text) in bodies {
        for callee in callees(text) {
            // Several functions may share a name (statics in different files): prefer the
            // caller's own file, else the only one there is.
            let Some(candidates) = by_name.get(callee) else { continue };
            let same = candidates.iter().find(|(_, m)| *m == module[&caller]);
            let target = match (same, candidates.as_slice()) {
                (Some((id, _)), _) => *id,
                (None, [(id, _)]) => *id,
                _ => continue,
            };
            if target != caller && seen.insert((caller, target)) {
                out.push((caller, target));
            }
        }
    }
    out
}

/// Names followed by `(` in preprocessed C `text`, in order, outside string and character
/// literals and not member accesses (`s.f(`, `p->f(`). A function's own name in its definition
/// is one of them; the caller drops self-calls.
fn callees(text: &str) -> impl Iterator<Item = &str> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut member = false;
    std::iter::from_fn(move || {
        while i < b.len() {
            let c = b[i];
            if c == b'"' || c == b'\'' {
                i += 1;
                while i < b.len() && b[i] != c {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
                member = false;
            } else if c.is_ascii_alphabetic() || c == b'_' {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let after = member;
                member = false;
                let mut j = i;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if !after && b.get(j) == Some(&b'(') {
                    return Some(&text[start..i]);
                }
            } else if c.is_ascii_digit() {
                // A number, suffix included (`10u`), is no name.
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                    i += 1;
                }
                member = false;
            } else {
                if !c.is_ascii_whitespace() {
                    member = c == b'.' || (c == b'>' && i > 0 && b[i - 1] == b'-');
                }
                i += 1;
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use argo_c::Parser;

    #[test]
    fn direct_calls_between_program_functions_are_found() {
        let c = "int leaf(int x) { return x + 1; }\n\
                 int mid(int x) { return leaf(x) * 2; }\n\
                 int top(int x) { int (*f)(int) = leaf; return mid(x) + f(x) + mid(1) + top(0); }\n";
        let parsed = Parser::default().parse(c.as_bytes());
        assert!(!parsed.failed());
        let graph = main_export(&parsed, c);
        let snap = Snapshot::new(graph.0, vec![graph.1]);
        let name = |id: NodeId| snap.graph.nodes.iter().find(|n| n.id == id).unwrap().name.clone();
        let pairs: Vec<(String, String)> = calls(&snap).into_iter().map(|(a, b)| (name(a), name(b))).collect();
        assert_eq!(
            pairs,
            [("mid".to_string(), "leaf".to_string()), ("top".to_string(), "mid".to_string())],
            "once per pair; the call through `f` and the self-call are left out"
        );
    }

    #[test]
    fn calls_are_found_without_the_typedefs_a_recorded_source_leaves_out() {
        // Parsed with its typedefs, then recorded without them (blanked, so offsets hold), as a
        // unit's source is when the typedefs came from a system header or another unit.
        let prelude = "typedef unsigned long size_t; typedef struct ops { int (*leaf)(int); } ops;\n";
        let body = "size_t leaf(size_t x) { return x + 1; }\n\
                    size_t top(ops *p, ops s) { char *t = \"leaf(\"; return leaf(2) + p->leaf(3) + s.leaf(4) + t[0]; }\n";
        let c = format!("{prelude}{body}");
        let parsed = Parser::default().parse(c.as_bytes());
        assert!(!parsed.failed());
        let (graph, source) = main_export(&parsed, &c);
        let blanked = format!("{}\n{body}", " ".repeat(prelude.len() - 1));
        let source = argo_protocol::SourceFile::new(source.id, source.path, blanked);
        assert!(Parser::default().parse(source.text.as_bytes()).failed(), "not parseable alone");
        let snap = Snapshot::new(graph, vec![source]);
        let name = |id: NodeId| snap.graph.nodes.iter().find(|n| n.id == id).unwrap().name.clone();
        let pairs: Vec<(String, String)> = calls(&snap).into_iter().map(|(a, b)| (name(a), name(b))).collect();
        assert_eq!(
            pairs,
            [("top".to_string(), "leaf".to_string())],
            "the members `p->leaf`/`s.leaf` and the string \"leaf(\" are no calls"
        );
    }

    #[test]
    fn callees_skip_literals_members_and_numbers() {
        let names: Vec<_> = callees(r#"int f(void) { g(1); x.h(2); y -> i(3); "j(" 'k'; l (4) ; 10u(5); }"#).collect();
        assert_eq!(names, ["f", "g", "l"]);
    }

    fn main_export(parsed: &argo_c::ParseResult, _c: &str) -> (argo_protocol::Graph, argo_protocol::SourceFile) {
        let graph = parsed.ast.build_graph(&parsed.source, argo_c::templates::all()).unwrap();
        argo_c::export::to_protocol(&graph, &parsed.ast, "t.c")
    }
}
