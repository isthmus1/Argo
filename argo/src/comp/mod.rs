use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod model;

pub use model::Replacement;

/// Swap the C function definition at `replacement.span` for its declaration,
/// link in the Rust that implements it, and run the program.
pub fn run_with_c(c: &str, replacement: &Replacement) -> Result<(), String> {
    let c = replace(c, replacement)?;
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("argo-comp-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("snippet.rs"), &replacement.rust).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("main.c"), c).map_err(|e| e.to_string())?;

    run(
        "rustc",
        Command::new("rustc")
            .args(["--edition=2024", "--crate-type=staticlib", "-o"])
            .arg(dir.join("libsnippet.a"))
            .arg(dir.join("snippet.rs")),
    )?;

    let host = Command::new("rustc")
        .args(["--print", "host-tuple"])
        .output()
        .map_err(|e| e.to_string())?;
    let host = String::from_utf8_lossy(&host.stdout);
    let compiler = cc::Build::new()
        .cargo_metadata(false)
        .target(host.trim())
        .host(host.trim())
        .opt_level(0)
        .try_get_compiler()
        .map_err(|e| e.to_string())?;
    run(
        "the C compiler",
        compiler
            .to_command()
            .arg(dir.join("main.c"))
            .arg(dir.join("libsnippet.a"))
            .arg("-o")
            .arg(dir.join("main")),
    )?;

    let result = run("the C program", &mut Command::new(dir.join("main")));
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn replace(c: &str, replacement: &Replacement) -> Result<String, String> {
    let span = replacement.span.clone();
    let function = c
        .get(span.clone())
        .ok_or_else(|| format!("{span:?} is not a valid range of the C source"))?;
    let (header, _) = function
        .split_once('{')
        .ok_or_else(|| format!("{span:?} is not a function definition"))?;
    Ok(format!(
        "{}{};{}",
        &c[..span.start],
        header.trim_end(),
        &c[span.end..]
    ))
}

fn run(what: &str, command: &mut Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|e| format!("cannot start {what}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = r#"
int add(int a, int b)
{
    return a + b;
}

int main(void) { return add(2, 3) == 5 ? 0 : 1; }
"#;

    /// Byte range of the `add` definition in [`C`].
    fn add_span() -> std::ops::Range<usize> {
        let start = C.find("int add").unwrap();
        let end = start + C[start..].find("}\n").unwrap() + 1;
        start..end
    }

    #[test]
    fn correct_rust_replaces_the_c_function() {
        let replacement = Replacement {
            span: add_span(),
            rust: r#"
#[unsafe(no_mangle)]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + b }
"#
            .to_string(),
        };
        assert_eq!(run_with_c(C, &replacement), Ok(()));
    }

    #[test]
    fn wrong_rust_fails_the_c_program() {
        let replacement = Replacement {
            span: add_span(),
            rust: r#"
#[unsafe(no_mangle)]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a - b }
"#
            .to_string(),
        };
        let err = run_with_c(C, &replacement).unwrap_err();
        assert!(err.contains("the C program failed"), "{err}");
    }

    #[test]
    fn the_definition_becomes_a_declaration() {
        let c = replace(
            C,
            &Replacement {
                span: add_span(),
                rust: String::new(),
            },
        )
        .unwrap();
        assert!(c.contains("int add(int a, int b);"));
        assert!(!c.contains("return a + b;"));
    }

    #[test]
    fn snippets_hold_each_nodes_text_and_range() {
        use crate::parser::CSnippet;
        use crate::{CNodeType, Lexer, Parser};
        use rucc_session::Std;

        let parser = Parser::new(Lexer::new("x86_64-unknown-linux-gnu", Std::C11, true).unwrap());
        let result = parser.parse(C.as_bytes());
        let add = result
            .ast
            .walk()
            .find(|n| n.node_type == CNodeType::Function && n.name.as_deref() == Some("add"))
            .unwrap();

        let snippet = CSnippet::from_node(add, &result.source).unwrap();

        assert_eq!(snippet.range, add_span());
        assert_eq!(snippet.text, &C[add_span()]);
    }
}
