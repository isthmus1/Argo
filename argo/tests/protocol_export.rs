use std::path::Path;

use argo_protocol::{Analysis, GraphIndex, NodeKind, Relation, ValueType, validate};
use main::{Parser, export, templates};

fn export(path: &str) -> argo_protocol::Graph {
    let parsed = Parser::default().parse_file(Path::new(path)).unwrap();
    assert!(!parsed.failed(), "{path} parses");
    let graph = parsed
        .ast
        .build_graph(&parsed.source, templates::all())
        .unwrap();
    export::to_protocol(&graph, &parsed.ast, path).0
}

#[test]
fn exported_graphs_pass_freeze_checks() {
    for path in [
        "../fixtures/compute/compute.c",
        "tests/fixture.c",
        "tests/fixtures/regions.c",
    ] {
        let mut graph = export(path);
        assert_eq!(validate(&graph), vec![], "{path}");
        assert_eq!(graph.revision.analysis, Analysis::SyntaxOnly);
        graph.freeze().unwrap();
    }
}

#[test]
fn function_pins_come_from_the_signature() {
    let graph = export("tests/fixture.c");
    let ix = GraphIndex::new(&graph);
    let ram_read = graph.nodes.iter().find(|n| n.name == "ram_read").unwrap();
    let pins: Vec<_> = ix
        .pins(ram_read.id)
        .map(|p| (p.key.to_string(), p.kind.value_type().map(ValueType::c)))
        .collect();
    let spelled = |s: &str| Some(s.to_string());
    assert_eq!(
        pins,
        [
            ("exec.in".into(), None),
            ("exec.then".into(), None),
            ("param.r".into(), spelled("struct ram *")),
            ("param.addr".into(), spelled("unsigned long")),
            ("param.w".into(), spelled("enum bus_width")),
            ("param.out".into(), spelled("unsigned long *")),
            ("return".into(), spelled("int")),
        ]
    );
    // `void ram_fill(...)` has no return pin; `static inline` isn't part of a type.
    let fill = graph.nodes.iter().find(|n| n.name == "ram_fill").unwrap();
    assert!(ix.pins(fill.id).all(|p| p.key.as_str() != "return"));
    let mask = graph.nodes.iter().find(|n| n.name == "mask_for").unwrap();
    let ret = ix
        .pins(mask.id)
        .find(|p| p.key.as_str() == "return")
        .unwrap();
    assert_eq!(ret.kind.value_type().unwrap().c(), "unsigned long");
}

#[test]
fn exits_escape_to_their_targets() {
    let graph = export("tests/fixtures/regions.c");
    let ix = GraphIndex::new(&graph);
    // `continue` inside the for body's `if` reaches the loop dispatch's backedge.
    let dispatch = graph
        .nodes
        .iter()
        .find(|n| {
            n.kind == NodeKind::Dispatch(argo_protocol::DispatchKind::Loop)
                && n.name.starts_with("for (")
        })
        .unwrap();
    let into_next: Vec<_> = graph
        .edges
        .iter()
        .filter_map(|e| match &e.relation {
            Relation::Exec { from, to }
                if to.node == dispatch.id && to.key.as_str() == "exec.next" =>
            {
                Some(from.key.to_string())
            }
            _ => None,
        })
        .collect();
    assert!(
        into_next.contains(&"exit.continue".to_string()),
        "{into_next:?}"
    );
    // Loops capture break/continue; the switch keeps the default arm's return.
    let switch = graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Switch)
        .unwrap();
    let keys: Vec<_> = ix.pins(switch.id).map(|p| p.key.to_string()).collect();
    assert_eq!(keys, ["exec.in", "exec.then", "exit.return"]);
    for n in graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Loop(_)))
    {
        assert!(
            ix.pins(n.id).all(|p| !p.key.as_str().starts_with("exit.")),
            "{}",
            n.name
        );
    }
}

#[test]
fn syntax_only_export_claims_no_data_flow() {
    let graph = export("../fixtures/compute/compute.c");
    assert!(
        graph
            .edges
            .iter()
            .all(|e| matches!(e.relation, Relation::Exec { .. }))
    );
    assert!(graph.nodes.iter().all(|n| n.effects.is_empty()));
}

#[test]
fn a_run_of_declarations_only_is_not_a_work_unit() {
    let graph = export("tests/fixtures/declarations.c");
    let unit = |name: &str| {
        let n = graph.nodes.iter().find(|n| n.name == name).unwrap();
        (n.template.as_str().to_string(), n.work_unit)
    };
    assert_eq!(
        unit("int *p; int i = 0; int t;"),
        ("c.declarations".into(), false),
        "its locals are used after it, so it can't become a function of its own"
    );
    assert_eq!(
        unit("p = &a; int n = g(a); t = n + i; return t + *p;"),
        ("c.sequence".into(), true),
        "a declaration among statements doesn't stop a run from being a work unit"
    );
}
