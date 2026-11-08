use main::{Parser, templates};

#[test]
fn graph_serialization_is_deterministic_and_retains_non_utf8_source_bytes() {
    let source = b"/*\xff*/ int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    assert_eq!(
        graph,
        parsed.ast.build_graph(source, templates::all()).unwrap()
    );
    let value: serde_json::Value = serde_json::from_str(&graph.to_json().unwrap()).unwrap();
    let retained: Vec<u8> = serde_json::from_value(value["source"].clone()).unwrap();
    assert_eq!(retained, source);
    assert_eq!(value["analysis"], "syntax_only");
}

#[test]
fn graph_owns_its_source_and_names_after_parser_ast_and_input_are_dropped() {
    let graph = {
        let source = String::from("int owned(void) { return 0; }");
        let parsed = Parser::default().parse(source.as_bytes());
        parsed
            .ast
            .build_graph(&parsed.source, templates::all())
            .unwrap()
    };
    assert_eq!(graph.nodes[graph.roots[0]].name.as_deref(), Some("owned"));
    assert_eq!(graph.source, b"int owned(void) { return 0; }");
    assert!(graph.to_json().unwrap().contains("owned"));
}
