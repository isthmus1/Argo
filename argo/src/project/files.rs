//! Finding a project's sources and naming them relative to its root.

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

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

pub fn short(file: &str, root: &Path) -> String {
    lines::canonical(file).map_or_else(|| file.to_string(), |p| relative(root, &p))
}
