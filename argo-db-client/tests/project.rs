//! Rows of a real multi-file project: every chunk's location must be coherent with the files it
//! names. The snapshot's one source is the combined, preprocessed text, so its byte offsets mean
//! nothing in the repository; only `places` can say where a chunk came from.

use std::path::{Path, PathBuf};
use std::process::Command;

use argo_c::project::{self, Opened, Options};
use argo_db_client::{ChunkKind, Origin, rebuild, to_rows};
use argo_protocol::{FileId, Snapshot};

fn sample() -> PathBuf {
    let root = std::env::temp_dir().join(format!("argo-db-rows-{}", std::process::id()));
    let files = [
        (
            "inc/util.h",
            "/* Shared helpers.\n * Several lines of comment shift every offset.\n */\n#ifndef UTIL_H\n#define UTIL_H\n#define SQUARE(x) ((x) * (x))\nstatic inline int square(int x) { return SQUARE(x); }\n#endif\n",
        ),
        (
            "a.c",
            "/* a.c: a header comment\n   spanning lines. */\n#include \"inc/util.h\"\n\nint twice(int x) {\n    return 2 * x;\n}\n",
        ),
        (
            "b.c",
            "// b.c\n#include \"inc/util.h\"\n\nint area(int w, int h)\n{\n    return SQUARE(w) + w * h;\n}\n",
        ),
    ];
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

fn rows(opened: &Opened) -> Vec<argo_db_client::CChunk> {
    let id = opened.snapshot.sources[0].id;
    let places = |file: FileId, pos: u32| {
        let (path, line) = opened.places.at(pos).filter(|_| file == id)?;
        Some((path.to_string(), line))
    };
    let origin = Origin {
        locator: opened.root.display().to_string(),
        commit: None,
        units: vec![],
        flags: opened.flags.clone(),
    };
    to_rows(&opened.snapshot, &origin, &places).unwrap()
}

/// Lines `lo..=hi` (1-based) of `root/path`.
fn lines(root: &Path, path: &str, lo: u32, hi: u32) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(path)).unwrap();
    let all: Vec<_> = text.lines().map(str::to_string).collect();
    all[lo as usize - 1..hi as usize].to_vec()
}

#[test]
fn chunk_locations_point_into_the_files_they_name() {
    if !Command::new("cc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skipped: no `cc`");
        return;
    }
    let root = sample();
    let opened = project::open(&root, "two", &Options::default(), &|_| {}).unwrap();
    let rows = rows(&opened);
    let ingested = &opened.snapshot.sources[0];

    let function = |name: &str| {
        let r = rows
            .iter()
            .find(|r| r.kind == ChunkKind::Function && r.name == name);
        let r = r.unwrap_or_else(|| panic!("no function {name}"));
        (r.path.as_str(), r.line_start, r.line_end)
    };
    assert_eq!(function("twice"), ("a.c", 5, 7));
    assert_eq!(function("area"), ("b.c", 4, 7));
    assert_eq!(function("square"), ("inc/util.h", 7, 7));

    for r in rows
        .iter()
        .filter(|r| r.node.is_some() && !r.source.is_empty())
    {
        // The text and bytes index the ingested file, never the repository file.
        assert_eq!(r.ingested, ingested.path);
        assert_ne!(r.ingested, r.path);
        let (lo, hi) = (r.byte_lo as usize, r.byte_hi as usize);
        assert_eq!(r.source, ingested.text[lo..hi]);
        if r.kind == ChunkKind::Function {
            let original = lines(&root, &r.path, r.line_start, r.line_end);
            assert!(
                original[0].contains(&format!("{}(", r.name)),
                "{}: {original:?}",
                r.name
            );
            assert!(
                original.last().unwrap().ends_with('}'),
                "{}: {original:?}",
                r.name
            );
        }
    }
    let area = rows.iter().find(|r| r.name == "area").unwrap();
    assert!(area.source.contains("((w) * (w))"), "preprocessed text");

    let rebuilt = rebuild(&rows).unwrap();
    assert_eq!(
        rebuilt.snapshot,
        Snapshot {
            workers: vec![],
            ..opened.snapshot.clone()
        }
    );
}
