//! Who calls whom among a program's functions, for the whole-program view.
//!
//! The protocol graph records syntax only, with no call relations between functions, so this
//! parses the C the snapshot already carries (its preprocessed sources) and keeps every direct
//! call `f(...)` whose callee is a function of the program. Calls through pointers name no
//! function and are left out.

use std::collections::{HashMap, HashSet};

use argo_c::{CNode, CNodeType, Parser};
use argo_protocol::{NodeId, NodeKind, Snapshot};

/// Caller and callee, each once per pair, in source order. No self-calls.
pub fn calls(snap: &Snapshot) -> Vec<(NodeId, NodeId)> {
    let functions: Vec<_> = snap.graph.nodes.iter().filter(|n| n.kind == NodeKind::Function).collect();
    // A function node by where its definition starts, which is exact; by name for callees.
    let at: HashMap<(argo_protocol::FileId, u32), NodeId> = functions
        .iter()
        .filter_map(|n| n.provenance.as_ref().map(|p| ((p.range.file, p.range.lo), n.id)))
        .collect();
    let mut by_name: HashMap<&str, Vec<(NodeId, Option<NodeId>)>> = HashMap::new();
    for n in &functions {
        by_name.entry(n.name.as_str()).or_default().push((n.id, n.parent));
    }
    let module: HashMap<NodeId, Option<NodeId>> = functions.iter().map(|n| (n.id, n.parent)).collect();

    let parser = Parser::default();
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for source in &snap.sources {
        let parsed = parser.parse(source.text.as_bytes());
        for root in parsed.ast.walk().filter(|n| n.node_type == CNodeType::Function) {
            let Some(&caller) = at.get(&(source.id, root.span.lo)) else { continue };
            for callee in callees(root) {
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
    }
    out
}

/// Names called directly inside `function`: each call expression whose callee is a plain name.
fn callees(function: &CNode) -> impl Iterator<Item = &str> {
    function
        .walk()
        .filter(|n| n.node_type == CNodeType::Expression && n.name.as_deref() == Some("call"))
        .filter_map(|call| call.children.first())
        .filter(|callee| callee.node_type == CNodeType::Expression && callee.children.is_empty())
        .filter_map(|callee| callee.name.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn main_export(parsed: &argo_c::ParseResult, _c: &str) -> (argo_protocol::Graph, argo_protocol::SourceFile) {
        let graph = parsed.ast.build_graph(&parsed.source, argo_c::templates::all()).unwrap();
        argo_c::export::to_protocol(&graph, &parsed.ast, "t.c")
    }
}
