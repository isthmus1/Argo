//! Paths in preprocessor flags and messages, moved between the user's tree and the checkout that
//! is parsed: include directories inside the tree must come from the commit, and paths under the
//! checkout are recorded relative to it so the rows don't depend on where it lives.

use std::path::{Path, PathBuf};

/// An include flag naming a directory under `from` (relative ones resolve against the working
/// directory, as `cc` would), moved under `to`; others unchanged.
pub fn rerooted(flag: &str, from: &Path, to: &Path) -> String {
    let Some(dir) = flag.strip_prefix("-I").map(PathBuf::from) else {
        return flag.into();
    };
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    match dir.strip_prefix(from) {
        Ok(rel) => format!("-I{}", to.join(rel).display()),
        Err(_) => flag.into(),
    }
}

/// An include flag inside the checkout, relative to it (`-I<root>` is `-I.`); others unchanged.
pub fn unrooted_flag(flag: &str, root: &Path) -> String {
    match flag.strip_prefix("-I").map(Path::new) {
        Some(dir) if dir == root => "-I.".into(),
        Some(dir) => match dir.strip_prefix(root) {
            Ok(rel) => format!("-I{}", rel.display()),
            Err(_) => flag.into(),
        },
        None => flag.into(),
    }
}

/// A message (the preprocessor's, naming files by absolute path) with the checkout prefix cut.
pub fn unrooted(text: &str, root: &Path) -> String {
    text.replace(&format!("{}/", root.display()), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_paths_are_recorded_relative_to_it() {
        let root = Path::new("/tmp/argo-x-1-2");
        let flag = |f| unrooted_flag(f, root);
        assert_eq!(flag("-I/tmp/argo-x-1-2"), "-I.");
        assert_eq!(flag("-I/tmp/argo-x-1-2/include"), "-Iinclude");
        assert_eq!(flag("-I/usr/include"), "-I/usr/include");
        assert_eq!(flag("-I/tmp/argo-x-1-22"), "-I/tmp/argo-x-1-22");
        assert_eq!(flag("-DROOT=/tmp/argo-x-1-2"), "-DROOT=/tmp/argo-x-1-2");
        assert_eq!(
            unrooted("/tmp/argo-x-1-2/a.c:3:10: fatal error: x.h", root),
            "a.c:3:10: fatal error: x.h"
        );
        let to = Path::new("/data/checkout");
        let moved = |f| rerooted(f, root, to);
        assert_eq!(moved("-I/tmp/argo-x-1-2/inc"), "-I/data/checkout/inc");
        assert_eq!(moved("-I/tmp/argo-x-1-22"), "-I/tmp/argo-x-1-22");
        assert_eq!(moved("-DX=/tmp/argo-x-1-2/inc"), "-DX=/tmp/argo-x-1-2/inc");
    }
}
