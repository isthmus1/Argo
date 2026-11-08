use main::graph::{Graph, GraphNodeKind as Kind};
use main::{CNodeRole as Role, Parser, templates};

fn expand(source: &[u8]) -> Graph {
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed(), "{:?}", parsed.diagnostics);
    parsed.ast.build_graph(source, templates::all()).unwrap()
}

#[test]
fn stacked_labels_and_fallthrough_share_one_case_group() {
    let graph = expand(include_bytes!("fixtures/regions.c"));
    let switch = graph
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Switch)
        .unwrap();
    assert_eq!(switch.children.len(), 2);
    let groups: Vec<_> = switch.children.iter().map(|&id| &graph.nodes[id]).collect();
    assert!(groups.iter().all(|node| node.kind == Kind::CaseGroup));
    assert_eq!(
        groups[0]
            .parts
            .iter()
            .map(|part| part.role)
            .collect::<Vec<_>>(),
        [Role::CaseValue, Role::CaseValue, Role::CaseValue]
    );
    assert_eq!(groups[1].parts[0].role, Role::DefaultLabel);
}

#[test]
fn conditional_and_nested_loop_breaks_do_not_end_a_case_group() {
    for middle in ["if (x) break;", "for (;;) break;"] {
        let source = format!(
            "int f(int x) {{ switch (x) {{ case 0: {middle} case 1: break; }} return 0; }}"
        );
        let graph = expand(source.as_bytes());
        let switch = graph
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Switch)
            .unwrap();
        assert_eq!(switch.children.len(), 1, "{middle}");
        assert_eq!(graph.nodes[switch.children[0]].parts.len(), 2);
    }
}

#[test]
fn nested_switches_keep_their_own_labels() {
    let graph = expand(b"int f(int x) { switch (x) { case 0: switch (x) { case 1: break; default: break; } break; default: return 0; } return 1; }");
    let switches: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == Kind::Switch)
        .collect();
    assert_eq!(switches.len(), 2);
    assert!(switches.iter().all(|node| node.children.len() == 2));
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn unsupported_switch_shapes_stay_whole_opaque_leaves() {
    for source in [
        &b"void f(int x) { switch (x) { int y=0; case 0: break; } }"[..],
        &b"void f(int x) { switch (x) { if (x) { case 0: break; } } }"[..],
        &b"void f(int x) { switch (x) return; }"[..],
    ] {
        let graph = expand(source);
        assert!(!graph.nodes.iter().any(|node| node.kind == Kind::Switch));
        let opaque = graph
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Opaque)
            .unwrap();
        assert!(opaque.children.is_empty());
        assert!(opaque.span.source(source).unwrap().starts_with(b"switch"));
        assert!(
            graph
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.node == opaque.id)
        );
    }
}
