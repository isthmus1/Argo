use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod model;

pub use model::Replacement;

/// Compile one Rust snippet on its own, as the static library it would be linked in as. The
/// error is rustc's output. Nothing is linked, so `extern "C"` functions it calls needn't exist.
///
/// Edition 2021 accepts both `#[no_mangle]` and `#[unsafe(no_mangle)]`, and plain
/// `extern "C" { ... }` blocks, whichever the model writes.
pub fn compiles(rust: &str) -> Result<(), String> {
    let dir = scratch()?;
    std::fs::write(dir.join("snippet.rs"), rust).map_err(|e| e.to_string())?;
    let result = run(
        "rustc",
        Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=staticlib",
                "--crate-name=snippet",
                "-o",
            ])
            .arg(dir.join("libsnippet.a"))
            .arg(dir.join("snippet.rs")),
    );
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A fresh directory per call, so parallel calls don't overwrite each other's files.
fn scratch() -> Result<std::path::PathBuf, String> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("argo-comp-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Swap the C function definition at `replacement.span` for its declaration,
/// link in the Rust that implements it, and run the program.
pub fn run_with_c(c: &str, replacement: &Replacement) -> Result<(), String> {
    let c = replace(c, replacement)?;
    let dir = scratch()?;
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
        .compiler(crate::project::cpp::GCC)
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

/// The C compiler the `cc` crate finds for this machine. Outside a build script `cc` has no
/// TARGET variable, so name this machine's.
fn c_compiler() -> Result<cc::Tool, String> {
    let host = Command::new("rustc")
        .args(["--print", "host-tuple"])
        .output()
        .map_err(|e| e.to_string())?;
    let host = String::from_utf8_lossy(&host.stdout);
    cc::Build::new()
        .compiler(crate::project::cpp::GCC)
        .cargo_metadata(false)
        .target(host.trim())
        .host(host.trim())
        .opt_level(0)
        .try_get_compiler()
        .map_err(|e| e.to_string())
}

/// Swap `replacement` into the project's real C, at `replacement.file`'s byte span, and check
/// the result: the translation unit `unit` (a `.c` file under `root`, which includes `file` or
/// is it) compiles with the project's `flags`, the Rust compiles, it exports `symbol` and the C
/// no longer defines it, and the two link. Nothing under `root` is written: the changed file is
/// compiled from a copy, the unit too when it isn't that file.
pub fn check_in_project(
    root: &Path,
    unit: &Path,
    flags: &[String],
    replacement: &Replacement,
) -> Result<(), String> {
    let dir = scratch()?;
    let result = check_in(&dir, root, unit, flags, replacement);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn check_in(
    dir: &Path,
    root: &Path,
    unit: &Path,
    flags: &[String],
    r: &Replacement,
) -> Result<(), String> {
    let read = |p: &Path| {
        std::fs::read_to_string(root.join(p)).map_err(|e| format!("{}: {e}", p.display()))
    };
    let tree = dir.join("tree");
    let copy = |p: &Path, text: &str| {
        let to = tree.join(p);
        std::fs::create_dir_all(to.parent().unwrap_or(&tree)).map_err(|e| e.to_string())?;
        std::fs::write(&to, text).map_err(|e| e.to_string())
    };
    let changed = replace(&read(&r.file)?, r)?;
    copy(&r.file, &changed)?;
    if unit != r.file {
        copy(unit, &read(unit)?)?;
    }

    // The unit from the copy: a quoted include finds the changed file there first (beside the
    // copy, or by -iquote), everything else in the real tree. Relative project flags (`-Iinclude`)
    // resolve from `root`, the directory the compiler runs in.
    let dir_of = |base: &Path, p: &Path| base.join(p.parent().unwrap_or(Path::new("")));
    let compiler = c_compiler()?;
    let mut cc = compiler.to_command();
    cc.current_dir(root)
        .args(["-std=gnu2x", "-fPIC", "-c", "-w"])
        .arg("-iquote")
        .arg(dir_of(&tree, &r.file))
        .arg("-iquote")
        .arg(dir_of(root, unit))
        .args(flags)
        .arg(tree.join(unit))
        .arg("-o")
        .arg(dir.join("unit.o"));
    run("the C compiler", &mut cc)?;

    std::fs::write(dir.join("snippet.rs"), &r.rust).map_err(|e| e.to_string())?;
    run(
        "rustc",
        Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=staticlib",
                "--crate-name=snippet",
                "-o",
            ])
            .arg(dir.join("libsnippet.a"))
            .arg(dir.join("snippet.rs")),
    )?;

    // The Rust must provide the symbol the C now leaves to it.
    let defines = |file: &Path| -> Result<bool, String> {
        let out = Command::new("nm")
            .arg("-g")
            .arg(file)
            .output()
            .map_err(|e| format!("cannot run nm: {e}"))?;
        let mangled = format!("_{}", r.symbol);
        Ok(String::from_utf8_lossy(&out.stdout).lines().any(|l| {
            let mut f = l.split_whitespace().rev();
            let (name, kind) = (f.next(), f.next());
            matches!(kind, Some("T" | "t")) && (name == Some(&r.symbol) || name == Some(&mangled))
        }))
    };
    if !defines(&dir.join("libsnippet.a"))? {
        return Err(format!(
            "the Rust doesn't export `{}`: write it as `#[no_mangle] pub extern \"C\" fn {}`",
            r.symbol, r.symbol
        ));
    }
    if defines(&dir.join("unit.o"))? {
        return Err(format!("the C still defines `{}`", r.symbol));
    }

    // Link them as a shared object: the C's reference to the symbol resolves to the Rust, and
    // any clash between the two shows; the rest of the program may stay undefined.
    let mut link = compiler.to_command();
    link.arg("-shared")
        .arg(dir.join("unit.o"))
        .arg(dir.join("libsnippet.a"))
        .arg("-o")
        .arg(dir.join("check.so"));
    if cfg!(target_os = "macos") {
        link.arg("-Wl,-undefined,dynamic_lookup");
    }
    run("the linker", &mut link)
}

/// Byte range of lines `first..=last` (1-based) in `text`, newline of the last included.
pub fn line_span(text: &str, first: u32, last: u32) -> Option<std::ops::Range<usize>> {
    if first == 0 || last < first {
        return None;
    }
    let mut starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1));
    let lo = starts.nth(first as usize - 1)?;
    let hi = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .nth(last as usize)
        .unwrap_or(text.len());
    (lo < hi && hi <= text.len()).then_some(lo..hi)
}

/// `c` with the span replaced: by the replacement's C, or (a function definition) by the
/// function's declaration.
fn replace(c: &str, replacement: &Replacement) -> Result<String, String> {
    let span = replacement.span.clone();
    let function = c
        .get(span.clone())
        .ok_or_else(|| format!("{span:?} is not a valid range of the C source"))?;
    if let Some(call) = &replacement.c {
        return Ok(format!("{}{call}\n{}", &c[..span.start], &c[span.end..]));
    }
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
            file: "main.c".into(),
            c: None,
            symbol: "add".into(),
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
            file: "main.c".into(),
            c: None,
            symbol: "add".into(),
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
                file: "main.c".into(),
                span: add_span(),
                rust: String::new(),
                c: None,
                symbol: "add".into(),
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

        let snippet = CSnippet::from_node(add, &result.source, "main.c").unwrap();
        assert_eq!(snippet.file, "main.c");

        assert_eq!(snippet.range, add_span());
        assert_eq!(snippet.text, &C[add_span()]);
    }

    #[test]
    fn a_snippet_that_compiles_on_its_own() {
        let rust = r#"
extern "C" {
    fn helper(x: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    unsafe { helper(a) + b }
}
"#;
        assert_eq!(compiles(rust), Ok(()));
    }

    #[test]
    fn a_snippet_that_does_not_compile_reports_rustc_errors() {
        let rust = r#"
#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 { a + c }
"#;
        let err = compiles(rust).unwrap_err();
        assert!(err.contains("cannot find value `c`"), "{err}");
    }

    /// A small project: a header and a `.c` file that includes it.
    fn project() -> std::path::PathBuf {
        let root = scratch().unwrap();
        std::fs::create_dir_all(root.join("include")).unwrap();
        std::fs::write(
            root.join("include/sum.h"),
            "typedef unsigned int u32;\nu32 sum(const u32 *v, int n);\n",
        )
        .unwrap();
        std::fs::write(
            root.join("sum.c"),
            "#include \"sum.h\"\n\
             u32 sum(const u32 *v, int n)\n\
             {\n\
             \x20   u32 total = 0;\n\
             \x20   for (int i = 0; i < n; i++) total += v[i];\n\
             \x20   return total;\n\
             }\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn a_function_is_swapped_for_rust_in_the_real_file() {
        let root = project();
        let text = std::fs::read_to_string(root.join("sum.c")).unwrap();
        let mut r = Replacement {
            file: "sum.c".into(),
            span: line_span(&text, 2, 7).unwrap(),
            rust: "#[no_mangle]\npub extern \"C\" fn sum(v: *const u32, n: i32) -> u32 {\n\
                   (0..n as usize).map(|i| unsafe { *v.add(i) }).sum()\n}\n"
                .into(),
            c: None,
            symbol: "sum".into(),
        };
        let flags = vec!["-Iinclude".to_string()];
        assert_eq!(
            check_in_project(&root, Path::new("sum.c"), &flags, &r),
            Ok(())
        );
        // Misnamed: the C is left needing `sum`.
        r.rust = r.rust.replace("fn sum", "fn total");
        let err = check_in_project(&root, Path::new("sum.c"), &flags, &r).unwrap_err();
        assert!(err.contains("doesn't export `sum`"), "{err}");
    }

    #[test]
    fn a_piece_of_a_function_becomes_a_call_to_rust() {
        let root = project();
        let text = std::fs::read_to_string(root.join("sum.c")).unwrap();
        let mut r = Replacement {
            file: "sum.c".into(),
            span: line_span(&text, 5, 5).unwrap(),
            rust: "#[no_mangle]\npub extern \"C\" fn sum_loop(total: *mut u32, v: *const u32, n: i32) {\n\
                   for i in 0..n as usize { unsafe { *total += *v.add(i) } }\n}\n"
                .into(),
            c: Some("    void sum_loop(u32 *, const u32 *, int); sum_loop(&total, v, n);".into()),
            symbol: "sum_loop".into(),
        };
        let flags = vec!["-Iinclude".to_string()];
        assert_eq!(
            check_in_project(&root, Path::new("sum.c"), &flags, &r),
            Ok(())
        );
        // C that doesn't compile is reported by the C compiler.
        r.c = Some("    sum_loop(&total, v, n".into());
        let err = check_in_project(&root, Path::new("sum.c"), &flags, &r).unwrap_err();
        assert!(err.contains("the C compiler failed"), "{err}");
    }
}
