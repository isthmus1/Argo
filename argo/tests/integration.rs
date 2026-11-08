use main::parser::parse_source;

#[tokio::test]
async fn parses_a_function() {
    let c = r#"
int add(int a, int b) {
    return a + b;
}
"#;

    let report = parse_source("add.c", c.as_bytes()).expect("report");

    assert!(!report.failed, "{}", report.output);
    assert!(report.output.contains("diagnostics: 0 (0 fatal)"));
    assert!(report.output.contains("function"));
    assert!(report.output.contains("add"));
}

#[tokio::test]
async fn reports_a_syntax_error() {
    let c = r#"
int broken( {
"#;

    let report = parse_source("broken.c", c.as_bytes()).expect("report");

    assert!(report.failed, "{}", report.output);
    assert!(report.output.contains("--- diagnostics ---"));
}

/// Run with `cargo test -p Argo --test integration prints_the_ast -- --nocapture` to see the tree.
#[tokio::test]
async fn prints_the_ast_of_a_small_c_snippet() {
    let c = r#"
int add(int a, int b)
{
    if (a > b) {
        return a - b;
    }
    return a + b;
}
"#;

    let parsed = main::Parser::default().parse(c.as_bytes());
    assert!(!parsed.failed(), "{:?}", parsed.diagnostics);

    println!("--- C ---{c}--- AST (kind name [byte range]) ---");
    for root in &parsed.ast.roots {
        main::report::tree(root, 0);
    }

    let add = parsed.ast.walk().find(|n| n.name.as_deref() == Some("add"));
    assert_eq!(add.map(|n| n.node_type), Some(main::CNodeType::Function));
}
