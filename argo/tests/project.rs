//! A multi-file project through the system preprocessor into one protocol graph.

use std::path::{Path, PathBuf};
use std::process::Command;

use argo_protocol::{GraphIndex, NodeKind};
use main::project::{self, Options, git::GitRef};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn sample(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("argo-project-{tag}-{}", std::process::id()));
    write(
        &root,
        &[
            (
                "inc/shape.h",
                "#ifndef SHAPE_H\n#define SHAPE_H\n#define AREA(w, h) ((w) * (h))\nstruct shape { int w, h; };\nstatic inline int area(struct shape s) { return AREA(s.w, s.h); }\n#endif\n",
            ),
            (
                "main.c",
                "#include <stdio.h>\n#include \"inc/shape.h\"\nint main(void) {\n    struct shape s = {2, 3};\n    printf(\"%d\\n\", area(s));\n    return 0;\n}\n",
            ),
            (
                "util.c",
                "#include \"inc/shape.h\"\nint twice(int x) { return x * 2; }\n",
            ),
            (
                "limit.c",
                "#ifndef LIMIT\n#error LIMIT comes from the Makefile\n#endif\nint limit(void) { return LIMIT; }\n",
            ),
            ("broken.c", "#include <argo_definitely_missing.h>\nint z;\n"),
            ("Makefile", "CFLAGS=-O2 -DLIMIT=4\n"),
        ],
    );
    root
}

fn modules_and_functions(snap: &argo_protocol::Snapshot) -> Vec<(String, Vec<String>)> {
    let g = &snap.graph;
    let ix = GraphIndex::new(g);
    let mut out: Vec<(String, Vec<String>)> = g
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|m| {
            let fns = ix
                .children(Some(m.id))
                .iter()
                .filter_map(|&c| ix.node(c))
                .filter(|c| c.kind == NodeKind::Function)
                .map(|c| c.name.clone())
                .collect();
            (m.name.clone(), fns)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn files_combine_into_one_graph_with_a_module_per_original_file() {
    if !have("cc") {
        eprintln!("skipped: no `cc`");
        return;
    }
    let root = sample("local");
    let opened = project::open(&root, "sample", &Options::default(), &|_| {}).unwrap();

    let units: Vec<_> = opened
        .units
        .iter()
        .map(|u| (u.path.as_str(), u.skipped.is_some()))
        .collect();
    assert_eq!(
        units,
        [
            ("broken.c", true),
            ("limit.c", false),
            ("main.c", false),
            ("util.c", false)
        ]
    );
    assert!(
        opened.units[0]
            .skipped
            .as_deref()
            .unwrap()
            .contains("argo_definitely_missing.h")
    );
    assert!(
        opened.flags.contains(&"-DLIMIT=4".to_string()),
        "{:?}",
        opened.flags
    );

    // The header's inline function appears once (include guard), macros are expanded, and
    // nothing from system headers (stdio.h) becomes a module.
    let modules = modules_and_functions(&opened.snapshot);
    assert_eq!(
        modules,
        [
            ("inc/shape.h".to_string(), vec!["area".to_string()]),
            ("limit.c".into(), vec!["limit".into()]),
            ("main.c".into(), vec!["main".into()]),
            ("util.c".into(), vec!["twice".into()]),
        ]
    );
    let text = &opened.snapshot.sources[0].text;
    assert!(
        text.contains("((s.w) * (s.h))") && !text.contains("AREA("),
        "macros are expanded"
    );
    assert!(
        !text.lines().any(|l| l.trim_start().starts_with('#')),
        "no directives reach the parser"
    );
    let program = opened
        .snapshot
        .graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Program)
        .unwrap();
    assert_eq!(program.name, "sample");
}

#[test]
fn caller_flags_override_and_unblock_files() {
    if !have("cc") {
        return;
    }
    let root = sample("flags");
    std::fs::write(root.join("Makefile"), "").unwrap();
    let blocked = project::open(&root, "s", &Options::default(), &|_| {}).unwrap();
    assert!(
        blocked
            .units
            .iter()
            .any(|u| u.path == "limit.c"
                && u.skipped.as_deref().is_some_and(|w| w.contains("LIMIT")))
    );
    let opts = Options {
        flags: vec!["-DLIMIT=9".into()],
        ..Options::default()
    };
    let opened = project::open(&root, "s", &opts, &|_| {}).unwrap();
    assert!(
        opened
            .units
            .iter()
            .any(|u| u.path == "limit.c" && u.skipped.is_none())
    );
}

#[test]
fn a_git_reference_is_cloned_and_opened_at_its_revision() {
    if !have("cc") || !have("git") {
        return;
    }
    let root = sample("git");
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .current_dir(&root)
            .args(["-c", "user.name=argo", "-c", "user.email=argo@localhost"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "v1"]);
    git(&["tag", "v1"]);
    // A later commit adds a function the tagged revision doesn't have.
    std::fs::write(root.join("later.c"), "int later(void) { return 1; }\n").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "v2"]);

    let url = format!("file://{}", root.display());
    let at = |rev: &str| {
        let r = GitRef::parse(&format!("{url}#{rev}"));
        let opened = project::open_git(&r, &Options::default(), &|_| {}).unwrap();
        assert!(
            opened
                .root
                .starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap())
        );
        modules_and_functions(&opened.snapshot)
            .into_iter()
            .map(|(m, _)| m)
            .collect::<Vec<_>>()
    };
    assert!(!at("v1").contains(&"later.c".to_string()));
    assert!(at("HEAD").contains(&"later.c".to_string()));
}
