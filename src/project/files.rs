//! Finding a project's sources and the flags its Makefile passes the compiler.

use std::path::{Path, PathBuf};

use super::lines;

/// Every `.c` file under `root`, sorted, skipping hidden and build directories.
pub fn c_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.')
                    && !matches!(name.as_str(), "target" | "node_modules" | "build")
                {
                    stack.push(path);
                }
            } else if ft.is_file() && name.ends_with(".c") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// `-D`, `-U` and `-I` flags from a top-level Makefile's `CFLAGS`, `CPPFLAGS` and `DEFINES`
/// assignments. Anything using make syntax (`$(...)`) is skipped, as are conditionals: this is a
/// best-effort default that the caller's own flags override.
pub fn makefile_flags(root: &Path) -> Vec<String> {
    let Some(text) = ["GNUmakefile", "makefile", "Makefile"]
        .iter()
        .find_map(|f| std::fs::read_to_string(root.join(f)).ok())
    else {
        return vec![];
    };
    let mut flags = vec![];
    for line in text.lines() {
        let line = line.trim();
        let Some((var, value)) = line.split_once('=') else {
            continue;
        };
        let var = var.trim_end_matches(['+', ':', '?']).trim();
        if !matches!(var, "CFLAGS" | "CPPFLAGS" | "DEFINES" | "DEFS") {
            continue;
        }
        let mut tokens = value.split_whitespace();
        while let Some(t) = tokens.next() {
            if t.contains('$') || t.contains('\\') {
                continue;
            }
            let t = if t == "-I" {
                tokens.next().map(|p| format!("-I{p}"))
            } else {
                Some(t.to_string())
            };
            let Some(t) = t else { continue };
            if let Some(p) = t.strip_prefix("-I") {
                let p = Path::new(p);
                let abs = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    root.join(p)
                };
                flags.push(format!("-I{}", abs.display()));
            } else if t.starts_with("-D") || t.starts_with("-U") {
                flags.push(t);
            }
        }
    }
    flags
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

pub fn short(file: &str, root: &Path) -> String {
    lines::canonical(file).map_or_else(|| file.to_string(), |p| relative(root, &p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makefile_flags_skip_make_syntax() {
        let dir = std::env::temp_dir().join(format!("argo-mk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Makefile"),
            "CFLAGS=-O2 -Wall -D_GNU_SOURCE -DVERSION=\\\"$(shell cat V)\\\"\nCFLAGS+=-DCONFIG_X -I inc\nLDFLAGS=-DNOT_THIS\n",
        )
        .unwrap();
        let flags = makefile_flags(&dir);
        assert_eq!(
            flags,
            [
                "-D_GNU_SOURCE".to_string(),
                "-DCONFIG_X".into(),
                format!("-I{}", dir.join("inc").display())
            ]
        );
    }
}
