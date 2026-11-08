use main::{CNodeType as Kind, Parser};

#[test]
fn declarations_distinguish_aliases_prototypes_objects_and_externs() {
    let parsed = Parser::default().parse(
        b"typedef unsigned long Size; int declared(int); int (*callback)(int); extern int ext; int x;",
    );
    assert!(!parsed.failed());
    let kinds: Vec<_> = parsed.ast.roots.iter().map(|node| node.node_type).collect();
    assert_eq!(
        kinds,
        [
            Kind::Typedef,
            Kind::Declaration,
            Kind::Variable,
            Kind::Declaration,
            Kind::Variable
        ]
    );
    let names: Vec<_> = parsed
        .ast
        .roots
        .iter()
        .map(|node| node.name.as_deref())
        .collect();
    assert_eq!(
        names,
        [
            Some("Size"),
            Some("declared"),
            Some("callback"),
            Some("ext"),
            Some("x")
        ]
    );
}

#[test]
fn combined_definitions_keep_both_the_type_and_its_bindings() {
    let parsed = Parser::default().parse(
        b"typedef struct Pair { int x; } Pair; struct Box { int y; } box;\
          union Value { int n; float f; }; enum Choice { A = 1, B };",
    );
    assert!(!parsed.failed());
    let kinds: Vec<_> = parsed.ast.roots.iter().map(|node| node.node_type).collect();
    assert_eq!(
        kinds,
        [
            Kind::Struct,
            Kind::Typedef,
            Kind::Struct,
            Kind::Variable,
            Kind::Union,
            Kind::Enum
        ]
    );
    assert_eq!(parsed.ast.roots[0].name.as_deref(), Some("Pair"));
    assert_eq!(parsed.ast.roots[0].children[0].name.as_deref(), Some("x"));
    assert_eq!(parsed.ast.roots[5].children[1].name.as_deref(), Some("B"));
}

#[test]
fn mixed_prototypes_and_objects_are_classified_individually() {
    let parsed = Parser::default().parse(b"int declared(void), object; extern int defined = 1;");
    assert!(!parsed.failed());
    let kinds: Vec<_> = parsed.ast.roots.iter().map(|node| node.node_type).collect();
    assert_eq!(kinds, [Kind::Declaration, Kind::Variable, Kind::Variable]);
}
