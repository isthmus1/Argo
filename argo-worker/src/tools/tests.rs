//! The tools read the clone and the session directory, and nothing else, whatever path,
//! `..` or symlink they are given.

use std::os::unix::fs::symlink;

use serde_json::json;
use tempfile::TempDir;

use super::Tools;

const SECRET: &str = "OUTSIDE-THE-ROOTS";

/// A clone with sources, a `.git` directory and links leading outside; a session directory
/// with a log; and an outside directory holding a secret.
struct Fixture {
    _base: TempDir,
    tools: Tools,
    session: std::path::PathBuf,
    outside: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let base = tempfile::tempdir().unwrap();
    let (repo, session, outside) = (
        base.path().join("repo"),
        base.path().join("session"),
        base.path().join("outside"),
    );
    for d in [
        &repo,
        &session,
        &outside,
        &repo.join("src"),
        &repo.join(".git"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(outside.join("secret.h"), format!("int {SECRET};\n")).unwrap();
    std::fs::write(repo.join(".git/config"), format!("{SECRET}\n")).unwrap();
    let lines: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    std::fs::write(repo.join("src/ten.c"), lines).unwrap();
    std::fs::write(repo.join("src/vm.h"), "struct VirtMachine { int ram; };\n").unwrap();
    std::fs::write(
        repo.join("main.c"),
        "#include \"src/vm.h\"\nint main(void) { return 0; }\n",
    )
    .unwrap();
    std::fs::write(
        session.join("rustc.log"),
        "error[E0425]: cannot find value `c`\n",
    )
    .unwrap();
    symlink(outside.join("secret.h"), repo.join("leak.h")).unwrap();
    symlink(&outside, repo.join("out")).unwrap();
    let tools = Tools::new(Some(&repo), &session).unwrap();
    Fixture {
        tools,
        session: session.canonicalize().unwrap(),
        outside,
        _base: base,
    }
}

#[test]
fn read_numbers_and_pages_lines() {
    let f = fixture();
    let out = f.tools.run(
        "read",
        &json!({"path": "src/ten.c", "offset": 3, "limit": 2}),
    );
    assert_eq!(
        out,
        "     3\tline 3\n     4\tline 4\n[lines 3-4 of 10; read with offset=5 for more]"
    );
    let all = f.tools.run("read", &json!({"path": "src/ten.c"}));
    assert!(all.ends_with("    10\tline 10\n"), "{all}");
    let past = f
        .tools
        .run("read", &json!({"path": "src/ten.c", "offset": 11}));
    assert!(past.starts_with("error: src/ten.c has 10 lines"), "{past}");
    let dir = f.tools.run("read", &json!({"path": "src"}));
    assert!(dir.contains("is a directory; use glob"), "{dir}");
}

#[test]
fn the_session_directory_is_read_by_absolute_path() {
    let f = fixture();
    let log = f.session.join("rustc.log");
    let out = f.tools.run("read", &json!({"path": log}));
    assert!(out.contains("cannot find value `c`"), "{out}");
    let found = f
        .tools
        .run("grep", &json!({"pattern": "E0425", "path": f.session}));
    assert_eq!(
        found,
        format!("{}:1: error[E0425]: cannot find value `c`\n", log.display())
    );
}

#[test]
fn nothing_outside_the_roots_is_reachable() {
    let f = fixture();
    let secret = f.outside.join("secret.h");
    let attempts = [
        ("read", json!({"path": "../outside/secret.h"})),
        ("read", json!({"path": secret})),
        ("read", json!({"path": "leak.h"})),
        ("read", json!({"path": "out/secret.h"})),
        ("read", json!({"path": "/etc/passwd"})),
        ("grep", json!({"pattern": SECRET, "path": "../outside"})),
        ("grep", json!({"pattern": SECRET, "path": f.outside})),
        ("glob", json!({"pattern": "**/*", "path": "out"})),
    ];
    for (tool, args) in attempts {
        let out = f.tools.run(tool, &args);
        assert!(!out.contains(SECRET), "{tool} {args} leaked: {out}");
        assert!(out.starts_with("error:"), "{tool} {args}: {out}");
        assert!(
            out.contains("outside the repository and the session directory"),
            "{out}"
        );
    }
    // A whole-repository search neither follows links out nor reads `.git`.
    let all = f.tools.run("grep", &json!({"pattern": SECRET}));
    assert_eq!(all, "no matches");
    let listed = f.tools.run("glob", &json!({"pattern": "**/*"}));
    assert!(
        !listed.contains("secret") && !listed.contains(".git"),
        "{listed}"
    );
}

#[test]
fn grep_filters_by_glob_and_case() {
    let f = fixture();
    let out = f.tools.run(
        "grep",
        &json!({"pattern": "virtmachine", "ignore_case": true}),
    );
    assert_eq!(out, "src/vm.h:1: struct VirtMachine { int ram; };\n");
    let only_c = f.tools.run(
        "grep",
        &json!({"pattern": "VirtMachine|vm\\.h", "glob": "*.c"}),
    );
    assert_eq!(only_c, "main.c:1: #include \"src/vm.h\"\n");
    let bad = f.tools.run("grep", &json!({"pattern": "("}));
    assert!(bad.starts_with("error: invalid pattern"), "{bad}");
    let missing = f.tools.run("grep", &json!({}));
    assert_eq!(missing, "error: missing string argument `pattern`");
}

#[test]
fn glob_lists_matching_files_sorted() {
    let f = fixture();
    assert_eq!(
        f.tools.run("glob", &json!({"pattern": "*.c"})),
        "main.c\nsrc/ten.c"
    );
    assert_eq!(
        f.tools.run("glob", &json!({"pattern": "src/*.h"})),
        "src/vm.h"
    );
    assert_eq!(
        f.tools
            .run("glob", &json!({"pattern": "*.h", "path": "src"})),
        "src/vm.h"
    );
    assert_eq!(
        f.tools.run("glob", &json!({"pattern": "*.rs"})),
        "no files match"
    );
}

#[test]
fn without_a_checkout_only_the_session_is_readable() {
    let f = fixture();
    let tools = Tools::new(None, &f.session).unwrap();
    let out = tools.run("read", &json!({"path": "main.c"}));
    assert!(out.contains("no repository checkout"), "{out}");
    let log = tools.run("read", &json!({"path": f.session.join("rustc.log")}));
    assert!(log.contains("E0425"), "{log}");
}
