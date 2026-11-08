use std::path::Path;
use std::process::{Command, Output};

fn run(path: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_main"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .output()
        .expect("parser CLI should start")
}

#[test]
fn lexical_errors_fail_without_emitting_nodes() {
    let output = run("tests/fixtures/lex_error.c");
    assert!(!output.status.success(), "fatal lexer errors must fail");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("stray '@'"));
    assert!(!stdout.contains("=== AST ==="));
}

#[test]
fn fixture_prints_the_template_hierarchy() {
    let output = run("tests/fixture.c");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("=== AST ==="));
    assert!(stdout.contains("Enum bus_width"));
    assert!(stdout.contains("Struct ram"));
    assert!(stdout.contains("Function ram_read"));
    assert!(stdout.contains("Switch"));
    assert!(stdout.contains("For"));
    assert!(stdout.contains("While"));
    assert!(stdout.contains("the tree written back out as C"));
}

#[test]
fn missing_input_fails_cleanly() {
    let output = run("tests/fixtures/does-not-exist.c");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot read")
    );
}
