use main::graph::{Graph, GraphEdgeKind, GraphNodeKind as Kind, NodeId};
use main::{Parser, templates};

const SOURCE: &[u8] = include_bytes!("fixtures/regions.c");

fn graph() -> Graph {
    let parsed = Parser::default().parse(SOURCE);
    assert!(!parsed.failed());
    parsed.ast.build_graph(SOURCE, templates::all()).unwrap()
}

fn hierarchy(graph: &Graph, id: NodeId, depth: usize, text: &mut String) {
    let node = &graph.nodes[id];
    text.push_str(&format!(
        "{}{:?} {}\n",
        "  ".repeat(depth),
        node.kind,
        node.template
    ));
    for &child in &node.children {
        hierarchy(graph, child, depth + 1, text);
    }
}

#[test]
fn expanded_fixture_hierarchy_matches_the_reviewable_snapshot() {
    let graph = graph();
    assert!(graph.diagnostics.is_empty());
    let mut actual = String::new();
    for &root in &graph.roots {
        hierarchy(&graph, root, 0, &mut actual);
    }
    assert_eq!(actual, include_str!("fixtures/regions.graph.txt"));
}

#[test]
fn identities_edges_and_parent_links_form_a_containment_forest() {
    let graph = graph();
    assert_eq!(graph.source, SOURCE);
    assert_eq!(graph.schema_version, 1);
    assert_eq!(graph.analysis, "syntax_only");
    assert_eq!(graph.edges.len(), graph.nodes.len() - graph.roots.len());
    for (id, node) in graph.nodes.iter().enumerate() {
        assert_eq!(id, node.id);
        assert!(node.span.source(SOURCE).is_some());
        let incoming: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.target == id)
            .collect();
        if let Some(parent) = node.parent {
            assert_eq!(incoming.len(), 1);
            assert_eq!(incoming[0].source, parent);
            assert_eq!(incoming[0].kind, GraphEdgeKind::Contains);
            assert!(graph.nodes[parent].children.contains(&id));
            assert!(graph.nodes[parent].span.lo <= node.span.lo);
            assert!(graph.nodes[parent].span.hi >= node.span.hi);
        } else {
            assert!(incoming.is_empty());
            assert!(graph.roots.contains(&id));
        }
    }
}

#[test]
fn original_fixture_has_types_and_functions_without_expression_node_explosion() {
    let source = include_bytes!("fixture.c");
    let parsed = Parser::default().parse(source);
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let roots: Vec<_> = graph.roots.iter().map(|&id| &graph.nodes[id]).collect();
    assert_eq!(
        roots.iter().filter(|node| node.kind == Kind::Type).count(),
        2
    );
    assert_eq!(
        roots
            .iter()
            .filter(|node| node.kind == Kind::Function)
            .count(),
        3
    );
    assert!(graph.nodes.len() < parsed.ast.walk().count() / 2);
    assert!(graph.diagnostics.is_empty());
}
