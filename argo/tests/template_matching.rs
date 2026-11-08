use main::templates::*;
use main::{CNode, CNodeType, Parser};

struct SpecificFunction;

impl Template for SpecificFunction {
    fn id(&self) -> &'static str {
        "specific-function"
    }
    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Function && node.name.as_deref() == Some("f")
    }
}

#[test]
fn concrete_template_vectors_are_accepted() {
    let parsed = Parser::default().parse(b"int f(void) { return 1; }");
    let matches = parsed.ast.match_templates(vec![FunctionTemplate]);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].template, "function");
    assert!(std::ptr::eq(matches[0].node, &parsed.ast.roots[0]));
}

#[test]
fn heterogeneous_templates_are_ordered_first_match_wins() {
    let parsed = Parser::default().parse(b"int f(void) { return 1; }");
    let templates: Vec<Box<dyn Template>> = vec![
        Box::new(SpecificFunction),
        Box::new(FunctionTemplate),
        Box::new(OpaqueTemplate),
    ];
    let before = parsed.ast.clone();
    let matches = parsed.ast.match_templates(templates);
    assert_eq!(matches[0].template, "specific-function");
    assert_eq!(matches.len(), parsed.ast.walk().count());
    assert_eq!(before, parsed.ast, "matching must not mutate syntax");
}

#[test]
fn no_matches_and_empty_lists_leave_the_tree_intact() {
    let parsed = Parser::default().parse(b"int x;");
    assert!(
        parsed
            .ast
            .match_templates(Vec::<FunctionTemplate>::new())
            .is_empty()
    );
    assert!(
        parsed
            .ast
            .match_templates(vec![FunctionTemplate])
            .is_empty()
    );
    assert_eq!(parsed.ast.roots.len(), 1);
}

#[test]
fn every_builtin_is_reexported_and_has_a_unique_identifier() {
    let templates = all();
    let mut ids: Vec<_> = templates.iter().map(|template| template.id()).collect();
    assert_eq!(ids.last(), Some(&"opaque"));
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
    let parsed = Parser::default().parse(include_bytes!("fixture.c"));
    assert!(!parsed.failed());
    let matches = parsed.ast.match_templates(templates);
    assert_eq!(matches.len(), parsed.ast.walk().count());
    for (matched, node) in matches.iter().zip(parsed.ast.walk()) {
        assert!(std::ptr::eq(matched.node, node));
    }
    for id in [
        "function", "struct", "enum", "if", "for", "while", "switch", "case", "return",
    ] {
        assert!(
            matches.iter().any(|matched| matched.template == id),
            "missing {id}"
        );
    }
}

#[test]
fn unsupported_nodes_are_not_silently_dropped() {
    let parsed = Parser::default().parse(b"int f(void) { asm(\"\"); return 0; }");
    assert!(!parsed.failed());
    let matches = parsed.ast.match_templates(all());
    assert!(
        matches
            .iter()
            .any(|matched| matched.template == "opaque"
                && matched.node.node_type == CNodeType::Opaque)
    );
}
