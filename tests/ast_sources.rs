use main::{CNode, CNodeType as Kind, Parser};
use rucc_diag::Span;

#[test]
fn labels_and_gotos_are_retained_without_claiming_graph_support() {
    let parsed = Parser::default().parse(b"int f(void) { again: goto again; }");
    assert!(!parsed.failed());
    for kind in [Kind::Label, Kind::Goto] {
        let node = parsed
            .ast
            .walk()
            .find(|node| node.node_type == kind)
            .unwrap();
        assert_eq!(node.name.as_deref(), Some("again"));
    }
}

#[test]
fn else_source_includes_the_keyword_and_intervening_comments() {
    let source = b"int f(int x) { if (x) return 1; /* retained */ else return 0; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let branch = parsed
        .ast
        .walk()
        .find(|node| node.node_type == Kind::Else)
        .unwrap();
    assert_eq!(
        branch.source(source).unwrap(),
        b" /* retained */ else return 0;"
    );
}

#[test]
fn source_slicing_checks_bounds_and_keeps_raw_bytes() {
    let node = CNode::new(Kind::Expression, Span::new(1, 3));
    assert_eq!(node.source(b"abcd"), Some(&b"bc"[..]));
    assert_eq!(node.source(b"a"), None);
    assert_eq!(node.source(b"\0\xff\xfe\0"), Some(&b"\xff\xfe"[..]));
}

#[test]
fn overlapping_declaration_spans_keep_shared_specifiers() {
    let source = b"struct Pair { int x; } a, b;";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    assert_eq!(parsed.ast.roots.len(), 3);
    let a = &parsed.ast.roots[1];
    let b = &parsed.ast.roots[2];
    assert_eq!(a.source(source).unwrap(), b"struct Pair { int x; } a");
    assert_eq!(b.source(source).unwrap(), b"struct Pair { int x; } a, b");
}
