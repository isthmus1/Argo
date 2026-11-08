use main::graph::GraphNodeKind as Kind;
use main::{Parser, templates};

#[test]
fn top_level_declarations_are_not_regions_but_local_declarations_stay_in_sequences() {
    let source = b"int declared(int); extern int external; int state; int f(void) { extern int external; int x=0; return x; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let names: Vec<_> = graph
        .roots
        .iter()
        .filter_map(|&id| graph.nodes[id].name.as_deref())
        .collect();
    assert_eq!(names, ["state", "f"]);
    let sequence = graph
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Sequence)
        .unwrap();
    let bytes = sequence.span.source(source).unwrap();
    assert!(bytes.starts_with(b"extern int external;"));
    assert_eq!(sequence.exits.len(), 1);
}

#[test]
fn variadic_callback_parameters_do_not_make_a_function_body_variadic() {
    let source = b"int f(int (*callback)(int, ...)) { return callback(1, 2); }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    assert_eq!(graph.nodes[graph.roots[0]].kind, Kind::Function);
    assert!(graph.diagnostics.is_empty());
}
