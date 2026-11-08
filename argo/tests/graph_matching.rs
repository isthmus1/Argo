use main::graph::{Expansion, GraphNodeKind as Kind, Unsupported};
use main::{CNode, Parser, Template, templates};

struct Decline;
impl Template for Decline {
    fn id(&self) -> &'static str {
        "decline"
    }
    fn matches(&self, _: &CNode) -> bool {
        true
    }
    fn expand<'a>(&self, _: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Err(Unsupported("deliberate rejection"))
    }
}

struct Accept;
impl Template for Accept {
    fn id(&self) -> &'static str {
        "custom"
    }
    fn matches(&self, _: &CNode) -> bool {
        true
    }
    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Ok(Expansion::leaf(Kind::Scope, node))
    }
}

#[test]
fn unsupported_expansion_falls_through_and_boxed_templates_forward_expansion() {
    let source = b"int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    let mut registry: Vec<Box<dyn Template>> = vec![Box::new(Decline)];
    registry.extend(templates::all());
    let graph = parsed.ast.build_graph(source, registry).unwrap();
    assert_eq!(graph.nodes[0].template, "function");
    assert!(graph.nodes.len() > 1);
    assert!(graph.diagnostics.is_empty());
}

#[test]
fn first_successful_expansion_wins_with_a_concrete_registry() {
    let source = b"int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    let graph = parsed
        .ast
        .build_graph(source, vec![Accept, Accept])
        .unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].template, "custom");
    assert_eq!(graph.nodes[0].kind, Kind::Scope);
}

#[test]
fn fallback_is_one_whole_source_retaining_leaf_with_the_rejection_reason() {
    let source = b"int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    let registry: Vec<Box<dyn Template>> =
        vec![Box::new(Decline), Box::new(templates::OpaqueTemplate)];
    let graph = parsed.ast.build_graph(source, registry).unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].kind, Kind::Opaque);
    assert!(graph.nodes[0].children.is_empty());
    assert_eq!(graph.nodes[0].span.source(source).unwrap(), source);
    assert!(
        graph.diagnostics[0]
            .message
            .contains("deliberate rejection")
    );
}
