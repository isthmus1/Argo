//! Building the C program a candidate is linked into, and reading linkers' complaints.

use argo_db_client::{CChunk, ChunkKind};
use tempfile::TempDir;

use super::{Build, Program, Toolchain, declaration, problems};

#[test]
fn the_declaration_drops_storage_specifiers() {
    assert_eq!(
        declaration("int add(int a) { return a; }").as_deref(),
        Some("int add(int a)")
    );
    assert_eq!(
        declaration("static inline uint32_t\nget(const S *s)\n{\n  return 0;\n}").as_deref(),
        Some("uint32_t\nget(const S *s)")
    );
    assert_eq!(declaration("int x = 1;"), None);
}

#[test]
fn gnu_ld_and_lld_complaints_are_read_by_symbol() {
    let gnu = "/usr/bin/ld: linked.o: in function `main':\n\
        linked.c:(.text+0x1e): undefined reference to `add'\n\
        /usr/bin/ld: libsnippet.a(snippet.o): in function `add':\n\
        snippet.rs:4: multiple definition of `helper'; linked.o:linked.c:3: first defined here\n\
        collect2: error: ld returned 1 exit status\n";
    let p = problems(gnu, true);
    assert_eq!(p.undefined.iter().collect::<Vec<_>>(), ["add"]);
    assert_eq!(p.duplicate.iter().collect::<Vec<_>>(), ["helper"]);
    assert!(p.other.is_empty(), "{:?}", p.other);
    let lld = "ld.lld: error: undefined symbol: helpr\n>>> referenced by snippet.rs:5\n\
        ld.lld: error: duplicate symbol: helper\n";
    let p = problems(lld, true);
    assert_eq!(p.undefined.iter().collect::<Vec<_>>(), ["helpr"]);
    assert_eq!(p.duplicate.iter().collect::<Vec<_>>(), ["helper"]);
}

fn tools() -> Toolchain {
    Toolchain {
        cc: std::env::var("CC").unwrap_or_else(|_| "cc".into()),
        libs: vec![],
    }
}

/// A repository of `files`, built with every `.c` file a unit.
fn built(files: &[(&str, &str)]) -> (TempDir, Build) {
    let repo = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(repo.path().join(name), text).unwrap();
    }
    let units: Vec<String> = files
        .iter()
        .map(|(n, _)| n.to_string())
        .filter(|n| n.ends_with(".c"))
        .collect();
    let build = Build::new(repo.path(), &units, &tools()).unwrap();
    (repo, build)
}

/// The function `name` of `text` (file `path`), starting at `starts`, as a chunk.
fn function(path: &str, text: &str, name: &str, starts: &str) -> CChunk {
    let lo = text.find(starts).unwrap();
    let hi = lo + text[lo..].find("}\n").unwrap() + 1;
    CChunk {
        id: 1,
        node: None,
        parent: None,
        name: name.into(),
        kind: ChunkKind::Function,
        translatable: true,
        depth: 0,
        descendants: 0,
        path: path.into(),
        line_start: 0,
        line_end: 0,
        ingested: "repo.i".into(),
        byte_lo: 0,
        byte_hi: 0,
        source: text[lo..hi].into(),
        records: String::new(),
        rust: String::new(),
        c_call: String::new(),
    }
}

/// The program for `chunk`, and the directory holding its files.
fn prepare(repo: &TempDir, build: &Build, chunk: &CChunk) -> (Program, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let p = Program::prepare(chunk, None, build, repo.path(), dir.path(), &tools());
    (p, dir)
}

#[test]
fn a_static_target_with_a_static_prototype_still_links() {
    let a = "static int f(int);\nint g(void) { return f(1); }\nstatic int f(int x) { return x; }\nint main(void) { return g(); }\n";
    let (repo, build) = built(&[("a.c", a)]);
    let (p, _dir) = prepare(
        &repo,
        &build,
        &function("a.c", a, "f", "static int f(int x)"),
    );
    assert_eq!(p.declaration, "int f(int x)");
    // C gives the plain declaration the earlier `static`'s linkage; the reference it leaves
    // undefined still resolves to the Rust export at link time.
    assert_eq!(p.objects.len(), 1, "{:?}", p.notes);
    assert!(
        p.symbols.local.contains("f"),
        "the original bindings are kept"
    );
    assert_eq!(p.baseline.undefined.iter().collect::<Vec<_>>(), ["f"]);
}

#[test]
fn macros_and_makefile_flags_are_applied_as_the_server_applied_them() {
    // The chunk's text is the preprocessed definition: found only if the unit is preprocessed
    // with the Makefile's flags.
    let a = "#define WIDTH_T SCALE_T\nWIDTH_T scale(WIDTH_T x) { return x * 2; }\nint main(void) { return 0; }\n";
    let makefile = "CFLAGS=-DSCALE_T=long\n";
    let (repo, build) = built(&[("a.c", a), ("Makefile", makefile)]);
    let text = "long scale(long x) { return x * 2; }\n";
    let (p, _dir) = prepare(&repo, &build, &function("a.c", text, "scale", "long scale"));
    assert_eq!(p.notes, Vec::<String>::new());
    assert_eq!(p.signature.unwrap().show(), "(x: long int) -> long int");
}

#[test]
fn checks_are_off_when_the_chunk_cannot_be_placed() {
    let a = "int f(int x) { return x; }\nint main(void) { return 0; }\n";
    let broken = "int g(void) { return undeclared; }\n";
    let (repo, build) = built(&[("a.c", a), ("broken.c", broken)]);
    assert_eq!(
        build.failures().map(|(p, _)| p).collect::<Vec<_>>(),
        ["broken.c"]
    );

    let moved = function("a.c", "int f(int y) { return y; }\n", "f", "int f");
    let (p, _dir) = prepare(&repo, &build, &moved);
    assert!(p.objects.is_empty() && p.signature.is_none());
    assert!(
        p.notes[0].contains("the chunk's text occurs 0 times in `a.c`"),
        "{:?}",
        p.notes
    );

    let nowhere = function("a.h", "int h(void) { return 9; }\n", "h", "int h");
    let (p, _dir) = prepare(&repo, &build, &nowhere);
    assert!(
        p.notes[0].contains("`a.h` is not a translation unit, and no unit holds"),
        "{:?}",
        p.notes
    );

    let (p, _dir) = prepare(&repo, &build, &function("broken.c", broken, "g", "int g"));
    assert!(
        p.notes[0].contains("`broken.c` doesn't compile here"),
        "{:?}",
        p.notes
    );
    assert!(p.notes[0].contains("undeclared"), "{:?}", p.notes);
}

#[test]
fn a_function_defined_in_a_header_is_placed_in_a_unit_that_includes_it() {
    let h = "static inline int clamp(int x) { return x < 0 ? 0 : x; }\n";
    let a = "#include \"util.h\"\nint use_a(int x) { return clamp(x); }\nint main(void) { return 0; }\n";
    let b = "#include \"util.h\"\nint use_b(int x) { return clamp(-x); }\n";
    let (repo, build) = built(&[("util.h", h), ("a.c", a), ("b.c", b)]);
    let (p, _dir) = prepare(
        &repo,
        &build,
        &function("util.h", h, "clamp", "static inline"),
    );
    assert_eq!(p.notes, Vec::<String>::new());
    assert_eq!(p.declaration, "int clamp(int x)");
    assert_eq!(p.objects.len(), 2);
    assert_eq!(p.signature.unwrap().show(), "(x: int) -> int");
    // Only the unit it was placed in calls the external symbol; the other keeps its copy.
    assert_eq!(p.baseline.undefined.iter().collect::<Vec<_>>(), ["clamp"]);
}
