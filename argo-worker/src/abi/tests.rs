//! Signatures read from objects that `cc` and `rustc` actually built, the way the worker builds
//! them (unlinked, with `-g`).

use std::path::Path;
use std::process::Command;

use super::{Class, Signature, compare, signature};

fn built(dir: &Path, cmd: &mut Command, out: &Path) -> Vec<u8> {
    let status = cmd.current_dir(dir).status().expect("compiler runs");
    assert!(status.success(), "{cmd:?}");
    std::fs::read(out).unwrap()
}

fn c(source: &str, symbol: &str) -> Signature {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.c"), source).unwrap();
    let out = dir.path().join("a.o");
    let obj = built(
        dir.path(),
        Command::new("cc")
            .args(["-g", "-O0", "-c", "a.c", "-o"])
            .arg(&out),
        &out,
    );
    signature(&obj, symbol).unwrap().expect("C signature")
}

fn rust(source: &str, symbol: &str) -> Signature {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), source).unwrap();
    let out = dir.path().join("lib.o");
    let obj = built(
        dir.path(),
        Command::new("rustc")
            .args([
                "--edition=2021",
                "--crate-type=staticlib",
                "-g",
                "-Ccodegen-units=1",
                "-Awarnings",
                "--emit=obj",
                "lib.rs",
                "-o",
            ])
            .arg(&out),
        &out,
    );
    signature(&obj, symbol).unwrap().expect("Rust signature")
}

const C: &str = "
#include <stddef.h>
#include <stdint.h>
typedef struct point { int x, y; } point;
typedef unsigned int u32;
long scale(const char *s, size_t n, u32 k, double f, point p, uint8_t flag) { return 0; }
static int helper(int a) { return a; }
int use_helper(void) { return helper(1); }
int sum(int n, ...) { return n; }
";

#[test]
fn c_signatures_resolve_typedefs_to_classes() {
    let sig = c(C, "scale");
    let classes: Vec<&Class> = sig.params.iter().map(|(_, v)| &v.class).collect();
    assert_eq!(
        classes,
        [
            &Class::Pointer,
            &Class::Int {
                size: 8,
                signed: Some(false)
            },
            &Class::Int {
                size: 4,
                signed: Some(false)
            },
            &Class::Float { size: 8 },
            &Class::Aggregate { size: 8 },
            &Class::Int {
                size: 1,
                signed: Some(false)
            },
        ]
    );
    assert_eq!(
        sig.ret.class,
        Class::Int {
            size: 8,
            signed: Some(true)
        }
    );
    assert_eq!(sig.params[0].0.as_deref(), Some("s"));
    assert_eq!(sig.params[0].1.spelled, "const char *");
    assert_eq!(sig.params[1].1.spelled, "size_t");
    assert!(!sig.variadic);
    assert!(c(C, "sum").variadic);
    assert_eq!(
        c(C, "helper").params.len(),
        1,
        "static functions are found too"
    );
}

#[test]
fn a_matching_rust_export_agrees() {
    let r = rust(
        r#"
use core::ffi::{c_char, c_double, c_long, c_uint};
#[repr(C)] pub struct Point { x: i32, y: i32 }
#[no_mangle]
pub unsafe extern "C" fn scale(s: *const c_char, n: usize, k: c_uint, f: c_double, p: Point, flag: u8) -> c_long { 0 }
"#,
        "scale",
    );
    assert_eq!(
        compare(&c(C, "scale"), &r),
        Vec::<String>::new(),
        "{}",
        r.show()
    );
}

#[test]
fn mismatched_widths_signedness_floats_and_returns_are_named() {
    let r = rust(
        r#"
#[repr(C)] pub struct Point { x: i32 }
#[no_mangle]
pub extern "C" fn scale(s: u64, n: u32, k: i32, f: f32, p: Point, flag: u8) {}
"#,
        "scale",
    );
    let diffs = compare(&c(C, "scale"), &r);
    let joined = diffs.join("\n");
    assert_eq!(diffs.len(), 6, "{joined}");
    assert!(
        joined.contains("parameter 1 `s`: C passes `const char *` (pointer)"),
        "{joined}"
    );
    assert!(joined.contains("parameter 2 `n`: C passes `size_t` (8-byte unsigned integer), the Rust export takes `u32` (4-byte unsigned integer)"), "{joined}");
    assert!(joined.contains("parameter 3 `k`"), "signedness: {joined}");
    assert!(joined.contains("parameter 4 `f`"), "float width: {joined}");
    assert!(joined.contains("parameter 5 `p`"), "struct size: {joined}");
    assert!(
        joined.contains("return value: C returns `long int` (8-byte signed integer)"),
        "{joined}"
    );
}

#[test]
fn arity_and_variadics_are_reported() {
    let r = rust(
        "#[no_mangle] pub extern \"C\" fn sum(n: i32) -> i32 { n }",
        "sum",
    );
    let diffs = compare(&c(C, "sum"), &r);
    assert!(diffs[0].contains("variadic"), "{diffs:?}");
    let r = rust(
        "#[no_mangle] pub extern \"C\" fn helper() -> i32 { 0 }",
        "helper",
    );
    assert_eq!(
        compare(&c(C, "helper"), &r),
        ["C takes 1 parameter(s), the Rust export takes 0"]
    );
}

#[test]
fn private_rust_functions_of_the_same_name_are_not_the_export() {
    let r = rust(
        r#"
mod inner { pub fn helper(a: u64, b: u64) -> u64 { a + b } }
#[no_mangle] pub extern "C" fn helper(a: i32) -> i32 { inner::helper(a as u64, 1) as i32 }
"#,
        "helper",
    );
    assert_eq!(r.params.len(), 1, "{}", r.show());
    assert_eq!(compare(&c(C, "helper"), &r), Vec::<String>::new());
}

#[test]
fn option_wrapped_pointers_and_bools_pass_as_c_does() {
    let csig = c(
        "#include <stdbool.h>\nint run(int (*cb)(int), bool on) { return on ? cb(1) : 0; }",
        "run",
    );
    let r = rust(
        r#"#[no_mangle] pub extern "C" fn run(cb: Option<extern "C" fn(i32) -> i32>, on: bool) -> i32 { 0 }"#,
        "run",
    );
    assert_eq!(
        compare(&csig, &r),
        Vec::<String>::new(),
        "{} vs {}",
        csig.show(),
        r.show()
    );
}
