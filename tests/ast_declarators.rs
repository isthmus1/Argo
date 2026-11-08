use main::{CNodeType as Kind, Parser};

#[test]
fn pointer_array_and_function_types_survive_lowering() {
    let parsed = Parser::default()
        .parse(b"int (*callback)(int); int values[4]; int *read(int p) { return 0; }");
    assert!(!parsed.failed());
    let type_names: Vec<_> = parsed
        .ast
        .walk()
        .filter(|node| node.node_type == Kind::Type)
        .filter_map(|node| node.name.as_deref())
        .collect();
    for prefix in ["pointer", "array", "function"] {
        assert!(
            type_names.iter().any(|name| name.starts_with(prefix)),
            "{prefix}"
        );
    }
    assert!(
        parsed
            .ast
            .walk()
            .any(|node| node.name.as_deref() == Some("4"))
    );
    assert!(
        parsed
            .ast
            .walk()
            .any(|node| node.node_type == Kind::Parameter && node.name.as_deref() == Some("p"))
    );
    // Pointer-to-function and function-returning-pointer have opposite roots.
    assert!(
        parsed.ast.roots[0].children[0]
            .name
            .as_deref()
            .unwrap()
            .starts_with("pointer")
    );
    assert!(
        parsed.ast.roots[2].children[0]
            .name
            .as_deref()
            .unwrap()
            .starts_with("function")
    );
}

#[test]
fn braced_initializers_and_bitfield_expressions_remain_in_the_tree() {
    let parsed =
        Parser::default().parse(b"struct Bits { unsigned flag: 3; }; int items[2] = { [1] = 9 };");
    assert!(!parsed.failed());
    assert!(
        parsed
            .ast
            .walk()
            .any(|node| node.node_type == Kind::Initializer)
    );
    for value in ["3", "1", "9"] {
        assert!(
            parsed
                .ast
                .walk()
                .any(|node| node.name.as_deref() == Some(value)),
            "{value}"
        );
    }
    for node in parsed.ast.walk() {
        assert!(
            node.source(&parsed.source).is_some(),
            "{:?}",
            node.node_type
        );
    }
}
