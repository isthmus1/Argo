//! The exact commit a worker must clone to see what was ingested.

use std::path::Path;
use std::process::Command;

/// `HEAD` of the checkout at `root`, when `root` is the top level of a clean git work tree. A
/// subdirectory of a repository, or a directory outside one, has no commit a worker could clone
/// to get the same paths, so it is recorded as having none (workers then prompt without
/// repository context). Uncommitted changes, untracked files included, are refused by default (a
/// deliberate addition to the contract): the commit is what gets parsed (see [`crate::checkout`]),
/// so they would silently be left out of what the user watches. `allow_dirty` ingests `HEAD`
/// anyway, without them. Ignored files don't count; the commit's own checkout never has them.
pub fn resolve(root: &Path, allow_dirty: bool) -> Result<Option<String>, String> {
    let without = |why: &str| {
        eprintln!(
            "argo-server: {} {why}; no commit recorded, so workers will prompt without repository \
             context",
            root.display()
        );
        Ok(None)
    };
    let Some(top) = git(root, &["rev-parse", "--show-toplevel"]) else {
        return without("is not a git checkout");
    };
    if std::fs::canonicalize(&top).ok().as_deref() != Some(root) {
        return without(&format!(
            "is inside the git checkout {top}, not its top level"
        ));
    }
    let Some(commit) = git(root, &["rev-parse", "HEAD"]) else {
        return without("has no commits");
    };
    if git(root, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()) {
        if !allow_dirty {
            return Err(format!(
                "{} has uncommitted changes, which ingesting {commit} would leave out; commit \
                 them, or pass --allow-dirty to ingest {commit} without them",
                root.display()
            ));
        }
        eprintln!(
            "argo-server: {} has uncommitted changes; ingesting {commit} without them \
             (--allow-dirty)",
            root.display()
        );
    }
    Ok(Some(commit))
}

/// A git command's trimmed stdout, if it succeeded.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> std::path::PathBuf {
        let dir = crate::scratch::dir(name);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.c"), "int a;\n").unwrap();
        dir
    }

    #[test]
    fn only_clean_top_level_checkouts_have_a_commit() {
        let dir = repo("commit");
        assert_eq!(resolve(&dir, false), Ok(None), "not a checkout yet");
        let run = |args: &[&str]| assert!(git(&dir, args).is_some(), "git {args:?}");
        run(&["init", "-q"]);
        run(&["add", "."]);
        assert_eq!(resolve(&dir, false), Ok(None), "no commits yet");
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "x",
        ]);
        let head = git(&dir, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(resolve(&dir, false), Ok(Some(head.clone())));
        assert_eq!(resolve(&dir.join("sub"), false), Ok(None));

        std::fs::write(dir.join("sub/b.c"), "int b;\n").unwrap();
        let refused = resolve(&dir, false).unwrap_err();
        assert!(refused.contains("uncommitted changes"), "{refused}");
        assert_eq!(
            resolve(&dir, true),
            Ok(Some(head)),
            "--allow-dirty still records HEAD"
        );
    }
}
