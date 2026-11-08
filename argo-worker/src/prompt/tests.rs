//! Context comes from inside the clone only, whatever path or symlink a row names.

use std::os::unix::fs::symlink;
use std::path::PathBuf;

use argo_db_client::{CChunk, ChunkKind};
use tempfile::TempDir;

use super::build;

const SECRET: &str = "OUTSIDE-THE-CLONE";

fn chunk(path: &str) -> CChunk {
    CChunk {
        id: 1,
        node: Some(1),
        parent: None,
        name: "add".into(),
        kind: ChunkKind::Function,
        translatable: true,
        depth: 1,
        descendants: 0,
        path: path.into(),
        line_start: 2,
        line_end: 2,
        ingested: path.into(),
        byte_lo: 0,
        byte_hi: 1,
        source: "int add(int a, int b) { return a + b; }".into(),
        records: "{}".into(),
    }
}

/// A clone holding `add.c`, an in-clone alias of it, and file and directory symlinks that lead
/// to a file outside it; both live as long as the returned directory.
fn clone() -> (TempDir, PathBuf) {
    let base = tempfile::tempdir().unwrap();
    let (root, outside) = (base.path().join("clone"), base.path().join("outside"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("leak.c"),
        format!("int {SECRET};\nint {SECRET};\n"),
    )
    .unwrap();
    let text = "// add\nint add(int a, int b) { return a + b; }\n";
    std::fs::write(root.join("add.c"), text).unwrap();
    for (link, target) in [
        ("alias.c", root.join("add.c")),
        ("leak.c", outside.join("leak.c")),
        ("out", outside.clone()),
    ] {
        symlink(target, root.join(link)).unwrap();
    }
    (base, root)
}

#[test]
fn context_is_read_from_the_clone() {
    let (_base, root) = clone();
    for path in ["add.c", "alias.c"] {
        let prompt = build(&chunk(path), Some(&root), None);
        assert!(
            prompt.contains("CONTEXT") && prompt.contains("// add"),
            "{path}: {prompt}"
        );
    }
}

#[test]
fn nothing_outside_the_clone_reaches_the_prompt() {
    let (_base, root) = clone();
    for path in ["leak.c", "out/leak.c", "../outside/leak.c"] {
        let prompt = build(&chunk(path), Some(&root), None);
        assert!(!prompt.contains(SECRET), "{path} leaked: {prompt}");
        assert!(!prompt.contains("CONTEXT"), "{path}");
    }
}
