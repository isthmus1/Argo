use main::graph::GraphNodeKind as Kind;
use main::{Parser, templates};

#[test]
fn labels_and_gotos_stay_opaque_instead_of_inventing_executable_edges() {
    let source = b"int f(void) { again: goto again; return 0; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let opaque: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == Kind::Opaque)
        .collect();
    assert_eq!(opaque.len(), 1);
    assert!(opaque[0].children.is_empty());
    assert_eq!(
        opaque[0].span.source(source).unwrap(),
        b"again: goto again;"
    );
    assert_eq!(graph.diagnostics.len(), 1);
}

#[test]
fn variadic_bodies_are_opaque_in_the_first_graph_catalog() {
    let source = b"int f(int n, ...) { return n; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].kind, Kind::Opaque);
    assert!(graph.diagnostics[0].message.contains("variadic"));
}

#[test]
fn unsupported_control_headers_keep_the_entire_branch_opaque() {
    let source = b"int f(void) { if (({ int x=1; x; })) return 1; return 0; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    assert!(!graph.nodes.iter().any(|node| node.kind == Kind::Branch));
    let opaque = graph
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Opaque)
        .unwrap();
    assert!(opaque.span.source(source).unwrap().starts_with(b"if"));
    assert!(opaque.children.is_empty());
}

#[test]
fn unsupported_statement_expressions_keep_the_whole_sequence() {
    let source = b"int f(void) { int x=0; x=({ int y=2; y; }); return x; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let opaque = graph
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Opaque)
        .unwrap();
    assert!(opaque.children.is_empty());
    assert!(opaque.span.source(source).unwrap().starts_with(b"int x=0;"));
    assert!(opaque.span.source(source).unwrap().ends_with(b"return x;"));
}
