use main::{CNodeType as Kind, Parser};

#[test]
fn nested_control_syntax_keeps_its_original_hierarchy() {
    let parsed = Parser::default().parse(
        br#"
        int f(int x) {
            if (x) { for (int i = 0; i < x; ++i) { continue; } }
            else if (x < 0) { while (x) { break; } }
            else { do { --x; } while (x); }
            switch (x) { case 0: case 1: ++x; break; default: return x; }
            return 0;
        }
    "#,
    );
    assert!(!parsed.failed());
    let root = &parsed.ast.roots[0];
    assert_eq!(root.node_type, Kind::Function);
    let kinds: Vec<_> = root.walk().map(|node| node.node_type).collect();
    for kind in [
        Kind::If,
        Kind::Else,
        Kind::For,
        Kind::While,
        Kind::DoWhile,
        Kind::Switch,
        Kind::Case,
        Kind::Default,
        Kind::Return,
        Kind::Break,
        Kind::Continue,
    ] {
        assert!(kinds.contains(&kind), "missing {kind:?}");
    }
    assert_eq!(kinds.iter().filter(|&&kind| kind == Kind::If).count(), 2);
    let outer_if = root.walk().find(|node| node.node_type == Kind::If).unwrap();
    assert_eq!(outer_if.children.last().unwrap().node_type, Kind::Else);
    let switch = root
        .walk()
        .find(|node| node.node_type == Kind::Switch)
        .unwrap();
    assert_eq!(
        switch
            .walk()
            .filter(|node| node.node_type == Kind::Case)
            .count(),
        2
    );
}

#[test]
fn expression_operands_calls_and_original_spelling_survive() {
    let source = b"int f(int *p, int x) { return p[x + 1] ? f(p, x - 1) : 0; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    for name in ["index", "+", "?:", "call", "-"] {
        let node = parsed
            .ast
            .walk()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap();
        assert!(!node.children.is_empty());
        assert!(node.source(source).is_some());
    }
    let conditional = parsed
        .ast
        .walk()
        .find(|node| node.name.as_deref() == Some("?:"))
        .unwrap();
    assert_eq!(conditional.children.len(), 3);
    assert_eq!(
        conditional.source(source).unwrap(),
        b"p[x + 1] ? f(p, x - 1) : 0"
    );
}

#[test]
fn traversal_is_preorder_and_visits_every_owned_child_once() {
    let parsed = Parser::default().parse(b"int f(void) { return 1 + 2; }");
    let nodes: Vec<_> = parsed.ast.walk().collect();
    assert_eq!(nodes[0].node_type, Kind::Function);
    let plus = nodes
        .iter()
        .position(|node| node.name.as_deref() == Some("+"))
        .unwrap();
    assert_eq!(nodes[plus + 1].name.as_deref(), Some("1"));
    assert_eq!(nodes[plus + 2].name.as_deref(), Some("2"));
    let count = 1 + nodes.iter().map(|node| node.children.len()).sum::<usize>();
    assert_eq!(count, nodes.len());
}
