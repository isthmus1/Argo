use main::graph::{ChildRegion, Expansion, GraphError, GraphNodeKind, SourceSpan, Unsupported};
use main::{Ast, CNode, CNodeType, Parser, Template, templates};
use rucc_diag::Span;

struct InvalidExpansion(bool);
impl Template for InvalidExpansion {
    fn id(&self) -> &'static str {
        "invalid"
    }
    fn matches(&self, _: &CNode) -> bool {
        true
    }
    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        let mut expansion = Expansion::leaf(GraphNodeKind::Function, node);
        if self.0 {
            expansion.node.as_mut().unwrap().span = SourceSpan {
                lo: 0,
                hi: u32::MAX,
            };
        } else {
            expansion.children.push(ChildRegion::Sequence(Vec::new()));
        }
        Ok(expansion)
    }
}

#[test]
fn missing_templates_return_an_error_not_a_silently_incomplete_graph() {
    let source = b"int value;";
    let parsed = Parser::default().parse(source);
    let error = parsed
        .ast
        .build_graph(source, Vec::<Box<dyn Template>>::new())
        .unwrap_err();
    assert!(matches!(error, GraphError::NoTemplate(_)));
    assert!(error.to_string().contains("no graph template"));
}

#[test]
fn invalid_ast_and_expansion_spans_are_rejected_without_panicking() {
    let ast = Ast::new(vec![CNode::new(CNodeType::Opaque, Span::new(0, 99))]);
    assert!(matches!(
        ast.build_graph(b"short", templates::all()),
        Err(GraphError::InvalidSpan(_))
    ));
    let source = b"int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    assert!(matches!(
        parsed.ast.build_graph(source, vec![InvalidExpansion(true)]),
        Err(GraphError::InvalidSpan(_))
    ));
    assert_eq!(
        parsed
            .ast
            .build_graph(source, vec![InvalidExpansion(false)])
            .unwrap_err(),
        GraphError::EmptyRegion
    );
}

#[test]
fn empty_input_has_a_valid_empty_graph_and_needs_no_templates() {
    let parsed = Parser::default().parse(b"");
    let graph = parsed
        .ast
        .build_graph(b"", Vec::<Box<dyn Template>>::new())
        .unwrap();
    assert!(graph.nodes.is_empty());
    assert!(graph.roots.is_empty());
    assert!(graph.edges.is_empty());
}
