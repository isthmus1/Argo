//! [`BuildCheck`] against real C repositories: the program built unit by unit from the clone,
//! candidates compiled with rustc, checked against the program's symbols, linked into it and
//! ABI-compared, as `submit_rewrite` does.

use std::sync::Arc;

use argo_db_client::{CChunk, ChunkKind, Origin, Unit};
use tempfile::TempDir;

use super::{BuildCheck, Candidate, Failure, Ingest, Stage, Validator, Verdict};

/// `a.c` holds the chunk `add`, which calls `helper` (defined in `b.c`) and the `static`
/// function `twice`.
const A: &str = "\
struct pair { int a; long b; };
int helper(int x);
static int twice(int x) { return 2 * x; }
long add(struct pair *p, int k) { return p->a + p->b + helper(k) + twice(k); }
int main(void) { struct pair p = {1, 2}; return add(&p, 3) == 13 ? 0 : 1; }
";
const B: &str = "int helper(int x) { return x + 1; }\n";

fn chunk(a: &str) -> CChunk {
    let lo = a.find("long add").unwrap();
    let hi = lo + a[lo..].find("}\n").unwrap() + 1;
    CChunk {
        id: 1,
        node: Some(1),
        parent: None,
        name: "add".into(),
        kind: ChunkKind::Function,
        translatable: true,
        depth: 1,
        descendants: 0,
        path: "a.c".into(),
        line_start: 4,
        line_end: 4,
        ingested: "repo.i".into(),
        byte_lo: 0,
        byte_hi: 0,
        source: a[lo..hi].into(),
        records: String::new(),
        rust: String::new(),
        c_call: String::new(),
    }
}

/// A repository of `files` (name, text) and its ingest as the server records it: every file a
/// unit.
fn ingest(files: &[(&str, &str)]) -> (TempDir, Origin) {
    let repo = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(repo.path().join(name), text).unwrap();
    }
    let origin = Origin {
        locator: repo.path().display().to_string(),
        commit: Some("0".repeat(40)),
        units: files
            .iter()
            .map(|(name, _)| Unit {
                path: name.to_string(),
                skipped: None,
            })
            .collect(),
        flags: vec![],
    };
    (repo, origin)
}

struct Checked {
    check: BuildCheck,
    chunk: CChunk,
    program: Arc<crate::program::Program>,
    session: TempDir,
    _repo: TempDir,
}

/// The program for `add`, the first file's chunk.
async fn prepared(files: &[(&str, &str)]) -> Checked {
    let (check, warning) = BuildCheck::detect().expect("rustc runs");
    assert_eq!(warning, None, "a C compiler is needed for these tests");
    let (repo, origin) = ingest(files);
    let session = tempfile::tempdir().unwrap();
    let chunk = chunk(files[0].1);
    let ingest = Ingest {
        origin: &origin,
        checkout: repo.path(),
    };
    let program = check
        .prepare(&chunk, Some(ingest), &session.path().join("c-program"))
        .await
        .expect("a program");
    Checked {
        check,
        chunk,
        program: Arc::new(program),
        session,
        _repo: repo,
    }
}

const AB: &[(&str, &str)] = &[("a.c", A), ("b.c", B)];

impl Checked {
    async fn verdict(&self, n: u32, source: &str) -> Verdict {
        let dir = self.session.path().join(format!("submission-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("lib.rs");
        std::fs::write(&file, source).unwrap();
        let candidate = Candidate {
            chunk: &self.chunk,
            source,
            file: &file,
            dir: &dir,
            checkout: None,
            program: Some(self.program.clone()),
        };
        self.check.validate(&candidate).await
    }

    async fn failures(&self, n: u32, source: &str) -> Vec<Failure> {
        match self.verdict(n, source).await {
            Verdict::Failed(f) => f,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }
}

const PAIR: &str = "#[repr(C)] pub struct Pair { a: i32, b: i64 }\n";

const GOOD: &str = r#"
extern "C" { fn helper(x: i32) -> i32; }
fn twice(x: i32) -> i32 { 2 * x }
#[no_mangle]
pub unsafe extern "C" fn add(p: *mut Pair, k: i32) -> i64 {
    (*p).a as i64 + (*p).b + helper(k) as i64 + twice(k) as i64
}
"#;

fn stages(f: &[Failure]) -> Vec<Stage> {
    f.iter().map(|f| f.stage).collect()
}

#[tokio::test]
async fn the_program_is_built_unit_by_unit_with_signature_and_symbols() {
    let c = prepared(AB).await;
    let p = &c.program;
    assert_eq!(p.notes, Vec::<String>::new());
    assert_eq!(p.declaration, "long add(struct pair *p, int k)");
    assert_eq!(
        p.objects.len(),
        2,
        "b.c's object, and a.c with `add` replaced"
    );
    assert!(p.preprocessed.as_ref().is_some_and(|f| f.exists()));
    assert_eq!(
        p.signature.as_ref().unwrap().show(),
        "(p: struct pair *, k: int) -> long int"
    );
    assert!(p.symbols.local.contains("twice") && p.symbols.global.contains("helper"));
    // Without Rust, only `add` is missing from the link.
    assert!(p.baseline.failed);
    assert_eq!(p.baseline.undefined.iter().collect::<Vec<_>>(), ["add"]);
}

#[tokio::test]
async fn a_faithful_rewrite_links_and_matches_the_abi() {
    let c = prepared(AB).await;
    let Verdict::Unvalidated(log) = c.verdict(1, &format!("{PAIR}{GOOD}")).await else {
        panic!("expected acceptance");
    };
    assert!(
        log.contains("compiled with rustc; linked into the C program"),
        "{log}"
    );
    assert!(log.contains("matches the C ABI (2 parameters)"), "{log}");
    assert!(log.ends_with("not run or tested"), "{log}");
}

#[tokio::test]
async fn rustc_errors_come_back_with_the_full_log() {
    let c = prepared(AB).await;
    let f = c
        .failures(
            1,
            "#[no_mangle] pub extern \"C\" fn add() -> i64 { missing }",
        )
        .await;
    assert_eq!(stages(&f), [Stage::Compile]);
    assert!(
        f[0].notes[0].starts_with("error[E0425]: cannot find value `missing`"),
        "{:?}",
        f[0].notes
    );
    let log = f[0].log.as_deref().unwrap();
    assert!(
        log.contains("submission-1/lib.rs:1:"),
        "errors name the saved file: {log}"
    );
}

#[tokio::test]
async fn calling_a_static_c_function_is_explained_once() {
    let c = prepared(AB).await;
    let source = r#"
extern "C" { fn helper(x: i32) -> i32; fn twice(x: i32) -> i32; }
#[no_mangle]
pub unsafe extern "C" fn add(p: *mut Pair, k: i32) -> i64 {
    (*p).a as i64 + (*p).b + helper(k) as i64 + twice(k) as i64
}
"#;
    let f = c.failures(1, &format!("{PAIR}{source}")).await;
    assert_eq!(stages(&f), [Stage::Symbols], "{f:?}");
    assert!(
        f[0].notes[0].starts_with("`twice` is `static` in the C program"),
        "{:?}",
        f[0].notes
    );
}

#[tokio::test]
async fn redefining_a_c_function_is_explained_once() {
    let c = prepared(AB).await;
    let source = GOOD.replace(
        "extern \"C\" { fn helper(x: i32) -> i32; }",
        "#[no_mangle] pub extern \"C\" fn helper(x: i32) -> i32 { x + 1 }",
    );
    let f = c.failures(1, &format!("{PAIR}{source}")).await;
    assert_eq!(stages(&f), [Stage::Symbols], "{f:?}");
    assert!(f[0].notes[0].contains("also defines `helper`, which the C program already defines"));
}

#[tokio::test]
async fn an_undefined_symbol_fails_the_link_with_the_linker_log() {
    let c = prepared(AB).await;
    let source = r#"
extern "C" { fn helper(x: i32) -> i32; fn helpr(x: i32) -> i32; }
#[no_mangle]
pub unsafe extern "C" fn add(p: *mut Pair, k: i32) -> i64 {
    (*p).a as i64 + (*p).b + helper(k) as i64 + helpr(k) as i64
}
"#;
    let f = c.failures(1, &format!("{PAIR}{source}")).await;
    assert_eq!(stages(&f), [Stage::Link], "{f:?}");
    assert!(
        f[0].notes[0].starts_with("`helpr` is undefined"),
        "{:?}",
        f[0].notes
    );
    let log = f[0].log.as_deref().unwrap();
    let mut lines = log.lines();
    assert_eq!(lines.next(), Some("Lines about your Rust:"));
    assert!(
        lines
            .take(2)
            .any(|l| l.contains("undefined reference to `helpr'")),
        "{log}"
    );
}

#[tokio::test]
async fn an_abi_mismatch_links_but_is_rejected() {
    let c = prepared(AB).await;
    let source = r#"
extern "C" { fn helper(x: i32) -> i32; }
fn twice(x: i32) -> i32 { 2 * x }
#[no_mangle]
pub unsafe extern "C" fn add(p: *mut Pair, k: i64) -> i32 {
    ((*p).a as i64 + (*p).b + helper(k as i32) as i64 + twice(k as i32) as i64) as i32
}
"#;
    let f = c.failures(1, &format!("{PAIR}{source}")).await;
    assert_eq!(stages(&f), [Stage::Abi], "{f:?}");
    assert!(
        f[0].summary
            .contains("the C declaration is `long add(struct pair *p, int k)`")
    );
    assert_eq!(
        f[0].notes,
        [
            "parameter 2 `k`: C passes `int` (4-byte signed integer), the Rust export takes `i64` (8-byte signed integer)",
            "return value: C returns `long int` (8-byte signed integer), the Rust export returns `i32` (4-byte signed integer)",
        ]
    );
}

#[tokio::test]
async fn a_missing_export_is_named() {
    let c = prepared(AB).await;
    let source = GOOD.replace(
        "#[no_mangle]\npub unsafe extern \"C\" fn add",
        "pub unsafe extern \"C\" fn add",
    );
    let f = c.failures(1, &format!("{PAIR}{source}")).await;
    assert_eq!(stages(&f), [Stage::Symbols], "{f:?}");
    assert!(
        f[0].notes[0].starts_with("`add` is not exported"),
        "{:?}",
        f[0].notes
    );
}

#[tokio::test]
async fn the_c_programs_own_link_problems_are_not_held_against_the_rust() {
    // Another program's `main` in the same repository, and a library that isn't installed
    // here: both fail the link with or without Rust.
    let a = A.replace(
        "int main(void) {",
        "extern int lib_init(void);\nint main(void) { lib_init();",
    );
    let tool = "int main(void) { return 0; }\n";
    let c = prepared(&[("a.c", &a), ("b.c", B), ("tool.c", tool)]).await;
    let base = &c.program.baseline;
    assert!(
        base.undefined.contains("lib_init") && base.duplicate.contains("main"),
        "{base:?}"
    );
    let Verdict::Unvalidated(log) = c.verdict(1, &format!("{PAIR}{GOOD}")).await else {
        panic!("expected acceptance");
    };
    assert!(
        log.contains("adding no link errors to the 3 it has without the Rust"),
        "{log}"
    );
}

#[tokio::test]
async fn a_unit_that_does_not_compile_is_left_out_of_the_link() {
    let broken = "int broken(void) { return undeclared; }\n";
    let c = prepared(&[("a.c", A), ("b.c", B), ("broken.c", broken)]).await;
    assert_eq!(c.program.objects.len(), 2, "{:?}", c.program.notes);
    assert!(matches!(
        c.verdict(1, &format!("{PAIR}{GOOD}")).await,
        Verdict::Unvalidated(_)
    ));
}

#[tokio::test]
async fn compile_only_without_a_c_program() {
    let check = BuildCheck::compile_only();
    let chunk = chunk(A);
    let (repo, origin) = ingest(&[("a.c", A)]);
    let session = tempfile::tempdir().unwrap();
    let ingest = Ingest {
        origin: &origin,
        checkout: repo.path(),
    };
    assert!(
        check
            .prepare(&chunk, Some(ingest), session.path())
            .await
            .is_none()
    );
    let file = session.path().join("lib.rs");
    let source = format!("{PAIR}{GOOD}");
    std::fs::write(&file, &source).unwrap();
    let candidate = Candidate {
        chunk: &chunk,
        source: &source,
        file: &file,
        dir: session.path(),
        checkout: None,
        program: None,
    };
    assert_eq!(
        check.validate(&candidate).await,
        Verdict::Unvalidated(
            "compiled with rustc; not linked or ABI-checked: no C program to check against".into()
        )
    );
}
