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
