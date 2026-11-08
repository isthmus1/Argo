//! A multi-file project through the system preprocessor, one translation unit per `.c` file,
//! merged into one protocol graph.

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
fn each_file_is_its_own_unit_merged_into_one_graph_with_a_module_per_original_file() {
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

    // The header's inline function, which main.c and util.c both include, appears once; macros
    // are expanded, and nothing from system headers (stdio.h) becomes a module.
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
    // One recorded source per unit, holding only the project items it contributed: the shared
    // header's with the first unit that includes it, and no system header text.
    let sources: Vec<_> = opened
        .snapshot
        .sources
        .iter()
        .map(|s| s.path.as_str())
        .collect();
    assert_eq!(sources, ["limit.i", "main.i", "util.i"]);
    let main = &opened.snapshot.sources[1].text;
    assert!(
        main.contains("((s.w) * (s.h))") && !main.contains("AREA("),
        "macros are expanded"
    );
    assert!(
        !opened.snapshot.sources[2].text.contains("struct shape"),
        "{}",
        opened.snapshot.sources[2].text
    );
    for s in &opened.snapshot.sources {
        assert!(
            !s.text.contains("extern "),
            "no system header text: {}",
            s.text
        );
        assert!(
            !s.text.lines().any(|l| l.trim_start().starts_with('#')),
            "no directives reach the parser"
        );
    }
    let program = opened
        .snapshot
        .graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Program)
        .unwrap();
    assert_eq!(program.name, "sample");

    // Byte offsets index each unit's recorded text; places trace each function back to the
    // original file and line it starts on.
    for n in opened.snapshot.graph.nodes.iter() {
        let (NodeKind::Function, Some(p)) = (&n.kind, &n.provenance) else {
            continue;
        };
        let (file, line) = opened.places.at(p.range.file, p.range.lo).unwrap();
        let original = std::fs::read_to_string(root.join(file)).unwrap();
        let text = original.lines().nth(line as usize - 1).unwrap();
        assert!(
            text.contains(&format!("{}(", n.name)),
            "{file}:{line}: {text}"
        );
    }
}

/// What a compiler keeps apart because each file is its own translation unit: the same macro
/// with a different arity, the same `static` function, and a name that is a typedef in one file
/// and a variable in another; plus a header that expands differently in two units, and C23 `bool`
/// from `<stdbool.h>`.
#[test]
fn names_one_file_defines_mean_nothing_to_another() {
    if !have("cc") {
        return;
    }
    let root = std::env::temp_dir().join(format!("argo-project-apart-{}", std::process::id()));
    write(
        &root,
        &[
            (
                "shared.h",
                "#ifdef WIDE\nstatic inline int width(void) { return 64; }\n#else\nstatic inline int width(void) { return 32; }\n#endif\n",
            ),
            (
                "a.c",
                "#include \"shared.h\"\n#define say(x) ((x) + 1)\nstatic int helper(int v) { return say(v); }\ntypedef int count;\ncount a(void) { return helper(width()); }\n",
            ),
            (
                "b.c",
                "#include <stdbool.h>\n#define say(a, b, c, d) ((a) + (b) + (c) + (d))\nstatic int helper(int v) { return say(v, v, v, v); }\nint count = 3;\nbool b(void) { return helper(count) > 0; }\n",
            ),
            (
                "c.c",
                "#define WIDE\n#include \"shared.h\"\nint c(void) { return width(); }\n",
            ),
            (
                "d.c",
                "#include \"shared.h\"\nint d(void) { return width(); }\n",
            ),
        ],
    );
    let opened = project::open(&root, "apart", &Options::default(), &|_| {}).unwrap();
    let skipped: Vec<_> = opened
        .units
        .iter()
        .filter_map(|u| u.skipped.as_deref())
        .collect();
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(
        modules_and_functions(&opened.snapshot),
        [
            ("a.c".to_string(), vec!["helper".to_string(), "a".into()]),
            ("b.c".into(), vec!["helper".into(), "b".into()]),
            ("c.c".into(), vec!["c".into()]),
            ("d.c".into(), vec!["d".into()]),
            // Once per expansion: the narrow one a.c and d.c share, and c.c's wide one.
            ("shared.h".into(), vec!["width".into(), "width".into()]),
        ]
    );
    let text = |path: &str| {
        let s = opened
            .snapshot
            .sources
            .iter()
            .find(|s| s.path == path)
            .unwrap();
        s.text.as_str()
    };
    assert!(text("a.i").contains("return ((v) + 1);"), "{}", text("a.i"));
    assert!(
        text("b.i").contains("return ((v) + (v) + (v) + (v));"),
        "{}",
        text("b.i")
    );
    assert!(text("c.i").contains("return 64;") && !text("c.i").contains("return 32;"));
    // d.c's narrow width is a.c's, already taken; only d.c's own function is recorded with it.
    assert_eq!(text("d.i"), "int d(void) { return width(); }\n");

    // Every function traces back to its own file and line, in whichever unit recorded it.
    for n in &opened.snapshot.graph.nodes {
        let (argo_protocol::NodeKind::Function, Some(p)) = (&n.kind, &n.provenance) else {
            continue;
        };
        let (file, line) = opened.places.at(p.range.file, p.range.lo).unwrap();
        let original = std::fs::read_to_string(root.join(file)).unwrap();
        let at = original.lines().nth(line as usize - 1).unwrap();
        assert!(at.contains(&format!(" {}(", n.name)), "{file}:{line}: {at}");
    }
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

/// A small project shaped like TinyEMU (the README's golden target), which used to open with
/// every file skipped: a Makefile whose defines live in `ifdef` blocks and per-file rules, a
/// version string from `$(shell cat VERSION)`, and an optional feature needing a header that
/// isn't installed.
fn tinyemu_like(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("argo-project-{tag}-{}", std::process::id()));
    write(
        &root,
        &[
            ("VERSION", "2019-12-21\n"),
            (
                "Makefile",
                "# switches\nCONFIG_NET=y\nCONFIG_WIDE=y\n#CONFIG_WIN32=y\n\
                 CFLAGS=-O2 -Wall -D_GNU_SOURCE -DCONFIG_VERSION=\\\"$(shell cat VERSION)\\\"\n\
                 ifdef CONFIG_NET\nCFLAGS+=-DCONFIG_NET\nendif\n\
                 ifdef CONFIG_WIN32\nCFLAGS+=-DCONFIG_WIN32\nendif\n\
                 ifdef CONFIG_WIDE\nCFLAGS+=-DCONFIG_MAX_XLEN=128\nelse\nCFLAGS+=-DCONFIG_MAX_XLEN=64\nendif\n\
                 OBJS=cpu32.o cpu128.o machine.o\n\
                 cpu32.o: cpu.c\n\t$(CC) $(CFLAGS) -DMAX_XLEN=32 -c -o $@ $<\n\
                 cpu128.o: cpu.c\n\t$(CC) $(CFLAGS) -DMAX_XLEN=128 -c -o $@ $<\n\
                 %.o: %.c\n\t$(CC) $(CFLAGS) -c -o $@ $<\n",
            ),
            (
                "net.h",
                "#ifdef CONFIG_NET\n#include <argo_absent_ssl/aes.h>\n#endif\nint net_ready(void);\n",
            ),
            (
                "cpu.c",
                "#ifndef MAX_XLEN\n#error MAX_XLEN must be defined\n#endif\n\
                 #if CONFIG_MAX_XLEN != MAX_XLEN\n#error built for the wrong width\n#endif\n\
                 #ifdef CONFIG_WIN32\n#error not a win32 build\n#endif\n\
                 int cpu_xlen(void) { return MAX_XLEN; }\n",
            ),
            (
                "machine.c",
                "#include \"net.h\"\nconst char *machine_version(void) { return CONFIG_VERSION; }\n\
                 int machine_wide(void) { return CONFIG_MAX_XLEN; }\n",
            ),
            (
                "main.c",
                "int cpu_xlen(void);\nint main(void) { return cpu_xlen() == 128 ? 0 : 1; }\n",
            ),
            // Needs a header no switch removes; turning off CONFIG_MAX_XLEN would only trade the
            // missing header for an `#error`, so the reason stays the header.
            (
                "sdl.c",
                "#if CONFIG_MAX_XLEN != 128\n#error wrong width\n#endif\n#include <argo_absent_sdl/SDL.h>\n",
            ),
        ],
    );
    root
}

#[test]
fn makefile_conditionals_per_file_rules_and_optional_features() {
    if !have("cc") {
        return;
    }
    let root = tinyemu_like("tinyemu");
    let opened = project::open(&root, "tiny", &Options::default(), &|_| {}).unwrap();
    let units: Vec<_> = opened
        .units
        .iter()
        .map(|u| (u.path.as_str(), u.skipped.as_deref(), u.flags.clone()))
        .collect();
    assert_eq!(
        units,
        [
            ("cpu.c", None, vec!["-DMAX_XLEN=128".to_string()]),
            ("machine.c", None, vec!["-UCONFIG_NET".to_string()]),
            ("main.c", None, vec![]),
            (
                "sdl.c",
                Some("argo_absent_sdl/SDL.h: No such file or directory (sdl.c:4)"),
                vec![]
            ),
        ],
        "{:#?}",
        opened.units
    );
    assert!(
        opened
            .flags
            .contains(&"-DCONFIG_VERSION=\"2019-12-21\"".to_string()),
        "{:?}",
        opened.flags
    );
    assert!(
        !opened
            .flags
            .iter()
            .any(|f| f.contains("CONFIG_WIN32") || f.ends_with("=64")),
        "{:?}",
        opened.flags
    );
    let modules = modules_and_functions(&opened.snapshot);
    assert_eq!(
        modules,
        [
            ("cpu.c".to_string(), vec!["cpu_xlen".to_string()]),
            (
                "machine.c".into(),
                vec!["machine_version".into(), "machine_wide".into()]
            ),
            ("main.c".into(), vec!["main".into()]),
        ]
    );
    let text: String = opened
        .snapshot
        .sources
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        text.contains("return 128;"),
        "cpu.c is read with its rule's MAX_XLEN"
    );
    assert!(
        text.contains("return \"2019-12-21\";"),
        "the version comes from the VERSION file"
    );
}

#[test]
fn when_nothing_opens_the_error_lists_each_file_and_its_reason() {
    if !have("cc") {
        return;
    }
    let root = std::env::temp_dir().join(format!("argo-project-none-{}", std::process::id()));
    write(
        &root,
        &[
            ("a.c", "#include <argo_absent_header.h>\n"),
            ("b.c", "#error needs configuring\n"),
            ("c.c", "int f( { }\n"),
        ],
    );
    let Err(e) = project::open(&root, "none", &Options::default(), &|_| {}) else {
        panic!("nothing should open");
    };
    let lines: Vec<&str> = e.lines().collect();
    assert_eq!(
        lines[0],
        "None of the 3 .c files could be preprocessed and parsed."
    );
    assert!(
        lines[1].starts_with("a.c: argo_absent_header.h") && lines[1].ends_with("(a.c:1)"),
        "{e}"
    );
    assert_eq!(lines[2], "b.c: #error needs configuring (b.c:1)");
    assert!(
        lines[3].starts_with("c.c: ") && lines[3].ends_with("(c.c:1)"),
        "{e}"
    );
}

/// The golden project itself, from the network.
#[test]
#[ignore = "network: clones github.com/meerd/TinyEMU"]
fn tinyemu_opens_with_its_core_files() {
    if !have("cc") || !have("git") {
        return;
    }
    let r = GitRef::parse("https://github.com/meerd/TinyEMU");
    let opened = project::open_git(&r, &Options::default(), &|_| {}).unwrap();
    for u in opened.units.iter().filter(|u| u.skipped.is_some()) {
        eprintln!("skipped {}: {}", u.path, u.skipped.as_deref().unwrap());
    }
    let modules: Vec<String> = modules_and_functions(&opened.snapshot)
        .into_iter()
        .filter(|(_, f)| !f.is_empty())
        .map(|(m, _)| m)
        .collect();
    for core in [
        "riscv_cpu.c",
        "riscv_machine.c",
        "virtio.c",
        "machine.c",
        "temu.c",
        "softfp.c",
        "iomem.c",
        "cutils.c",
        "pci.c",
        "fs_disk.c",
        "slirp/slirp.c",
    ] {
        assert!(
            opened
                .units
                .iter()
                .any(|u| u.path == core && u.skipped.is_none()),
            "{core} included"
        );
        assert!(modules.iter().any(|m| m == core), "{core} has functions");
    }
    let riscv = opened
        .units
        .iter()
        .find(|u| u.path == "riscv_cpu.c")
        .unwrap();
    assert_eq!(riscv.flags, ["-DMAX_XLEN=128"]);
    let names: Vec<&str> = opened
        .snapshot
        .graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Function)
        .map(|n| n.name.as_str())
        .collect();
    for f in [
        "riscv_cpu_init",
        "riscv_cpu_interp_x64",
        "virt_machine_init",
    ] {
        assert!(names.contains(&f), "{f}");
    }
}
