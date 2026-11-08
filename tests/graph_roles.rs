use main::graph::{ControlExitKind as Exit, GraphNodeKind as Kind};
use main::{CNodeRole as Role, Parser, templates};

#[test]
fn omitted_for_clauses_do_not_shift_the_roles_of_remaining_clauses() {
    let source = b"void f(int n) { for (;;) break; for (;n;) --n; for (n=0;;n++) break; }";
    let parsed = Parser::default().parse(source);
    assert!(!parsed.failed());
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let loops: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == Kind::Loop)
        .collect();
    let roles: Vec<Vec<_>> = loops
        .iter()
        .map(|node| node.parts.iter().map(|part| part.role).collect())
        .collect();
    assert_eq!(
        roles,
        [
            vec![],
            vec![Role::Condition],
            vec![Role::Initializer, Role::Step]
        ]
    );
    for node in loops {
        assert_eq!(node.children.len(), 1);
        let body = &graph.nodes[node.children[0]];
        assert_eq!(body.kind, Kind::Sequence);
        assert_eq!(body.role, Some(Role::Body));
    }
}

#[test]
fn loop_headers_remain_on_loops_and_exits_remain_on_their_sequences() {
    let source = include_bytes!("fixtures/regions.c");
    let parsed = Parser::default().parse(source);
    let graph = parsed.ast.build_graph(source, templates::all()).unwrap();
    let loops: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.kind == Kind::Loop)
        .collect();
    assert_eq!(loops.len(), 3);
    assert_eq!(
        loops[0]
            .parts
            .iter()
            .map(|part| part.role)
            .collect::<Vec<_>>(),
        [Role::Initializer, Role::Condition, Role::Step]
    );
    for node in loops {
        assert_eq!(node.children.len(), 1);
        for part in &node.parts {
            assert!(part.span.source(source).is_some());
        }
    }
    let exits: Vec<_> = graph
        .nodes
        .iter()
        .flat_map(|node| {
            if !node.exits.is_empty() {
                assert_eq!(node.kind, Kind::Sequence);
            }
            node.exits.iter().map(|exit| exit.kind)
        })
        .collect();
    assert_eq!(
        exits,
        [
            Exit::Continue,
            Exit::Break,
            Exit::Break,
            Exit::Return,
            Exit::Return
        ]
    );
    let branch = graph
        .nodes
        .iter()
        .find(|node| node.kind == Kind::Branch)
        .unwrap();
    assert_eq!(branch.parts[0].role, Role::Condition);
    assert_eq!(
        branch
            .children
            .iter()
            .map(|&id| graph.nodes[id].role)
            .collect::<Vec<_>>(),
        [Some(Role::Then), Some(Role::Else)]
    );
}
