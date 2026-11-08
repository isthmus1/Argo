use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_main"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(arguments)
        .output()
        .expect("graph CLI should start")
}

#[test]
fn graph_mode_emits_only_json_and_keeps_original_source() {
    let output = run(&["--graph-json", "tests/fixture.c"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with('{'), "stdout must be a JSON document");
    let graph: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(graph["schema_version"], 1);
    assert_eq!(graph["analysis"], "syntax_only");
    let bytes: Vec<u8> = serde_json::from_value(graph["source"].clone()).unwrap();
    assert_eq!(bytes, include_bytes!("fixture.c"));
    assert!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "ram_read")
    );
    assert!(output.stderr.is_empty());
    assert!(!stdout.contains("=== AST ==="));
}

#[test]
fn graph_errors_leave_stdout_empty() {
    for path in [
        "tests/fixtures/lex_error.c",
        "tests/fixtures/does-not-exist.c",
    ] {
        let output = run(&["--graph-json", path]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn extra_arguments_are_rejected() {
    let output = run(&["--graph-json", "tests/fixture.c", "unexpected"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
