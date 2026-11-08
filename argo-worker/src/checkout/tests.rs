//! Clones of a scratch repository: exact revision, revision changes and refused local edits,
//! including edits made after the clone was prepared and ignored files.

use std::path::{Path, PathBuf};

use argo_db_client::Origin;
use tempfile::TempDir;

use super::{Checkout, git, prepare};
use crate::workdir::hold;

/// A fresh directory per test, gone when the test's `TempDir` drops.
fn scratch() -> TempDir {
    tempfile::tempdir().unwrap()
}

/// Commit `file` with `text` in `repo`; the new commit id.
fn commit(repo: &Path, file: &str, text: &str) -> String {
    std::fs::write(repo.join(file), text).unwrap();
    git(Some(repo), ["add", file]).unwrap();
    let who = [
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
    ];
    git(
        Some(repo),
        who.into_iter().chain(["commit", "--quiet", "-m", file]),
    )
    .unwrap();
    git(Some(repo), ["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

/// A repository with one commit under `base`; its path and that commit.
fn upstream(base: &TempDir) -> (PathBuf, String) {
    let repo = base.path().join("up");
    std::fs::create_dir(&repo).unwrap();
    git(Some(&repo), ["init", "--quiet"]).unwrap();
    let first = commit(&repo, "add.c", "int add(int a, int b) { return a + b; }\n");
    (repo, first)
}

fn origin(repo: &Path, commit: &str) -> Origin {
    Origin {
        locator: repo.display().to_string(),
        commit: Some(commit.into()),
        units: Vec::new(),
        flags: Vec::new(),
    }
}

fn head(dir: &Path) -> String {
    git(Some(dir), ["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

#[test]
fn a_clean_clone_is_reused_and_follows_a_new_commit() {
    let base = scratch();
    let (repo, first) = upstream(&base);
    let mut checkout = Checkout::new(hold(&base.path().join("clone")).unwrap());
    let dir = checkout.sync(&origin(&repo, &first)).unwrap().unwrap();
    assert_eq!(head(&dir), first);
    assert_eq!(
        checkout.sync(&origin(&repo, &first)).unwrap(),
        Some(dir.clone())
    );
    // A reset and re-ingest of a commit the clone has never seen.
    let second = commit(&repo, "sub.c", "int sub(int a, int b) { return a - b; }\n");
    checkout.sync(&origin(&repo, &second)).unwrap();
    assert_eq!(head(&dir), second);
    assert!(dir.join("sub.c").exists());
}

#[test]
fn modified_tracked_files_are_refused_not_discarded() {
    let base = scratch();
    let (repo, first) = upstream(&base);
    let dir = base.path().join("clone");
    prepare(&origin(&repo, &first), &dir).unwrap();
    std::fs::write(dir.join("add.c"), "int add(int a, int b) { return 0; }\n").unwrap();
    let err = prepare(&origin(&repo, &first), &dir).unwrap_err();
    assert!(
        err.contains("local changes") && err.contains("add.c"),
        "{err}"
    );
    let kept = std::fs::read_to_string(dir.join("add.c")).unwrap();
    assert!(kept.contains("return 0"), "the edit survives");
}

#[test]
fn staged_and_untracked_files_are_refused() {
    let base = scratch();
    let (repo, first) = upstream(&base);
    let dir = base.path().join("clone");
    prepare(&origin(&repo, &first), &dir).unwrap();
    std::fs::write(dir.join("extra.c"), "int extra;\n").unwrap();
    assert!(prepare(&origin(&repo, &first), &dir).is_err(), "untracked");
    git(Some(&dir), ["add", "extra.c"]).unwrap();
    assert!(prepare(&origin(&repo, &first), &dir).is_err(), "staged");
}

#[test]
fn option_like_origins_are_refused() {
    let base = scratch();
    let dir = base.path().join("clone");
    let mut bad = origin(Path::new("--upload-pack=touch x"), "abc123");
    assert!(prepare(&bad, &dir).is_err());
    bad = origin(Path::new("/repo"), "--orphan");
    assert!(prepare(&bad, &dir).is_err());
}

#[test]
fn a_prepared_clone_is_rechecked_on_every_reuse() {
    let base = scratch();
    let (repo, first) = upstream(&base);
    let second = commit(&repo, "sub.c", "int sub(int a, int b) { return a - b; }\n");
    let mut checkout = Checkout::new(hold(&base.path().join("clone")).unwrap());
    let at = origin(&repo, &first);
    let dir = checkout.sync(&at).unwrap().unwrap();
    std::fs::write(dir.join("add.c"), "int add(int a, int b) { return 0; }\n").unwrap();
    let err = checkout.sync(&at).unwrap_err();
    assert!(
        err.contains("local changes") && err.contains("add.c"),
        "{err}"
    );
    git(Some(&dir), ["checkout", "--quiet", "--", "add.c"]).unwrap();
    checkout.sync(&at).unwrap();
    git(Some(&dir), ["checkout", "--quiet", "--detach", &second]).unwrap();
    let err = checkout.sync(&at).unwrap_err();
    assert!(err.contains("moved to"), "{err}");
}

#[test]
fn ignored_files_are_local_changes() {
    let base = scratch();
    let (repo, _) = upstream(&base);
    let ignoring = commit(&repo, ".gitignore", "generated.c\n");
    let dir = base.path().join("clone");
    prepare(&origin(&repo, &ignoring), &dir).unwrap();
    std::fs::write(dir.join("generated.c"), "int generated;\n").unwrap();
    let err = prepare(&origin(&repo, &ignoring), &dir).unwrap_err();
    assert!(err.contains("generated.c"), "{err}");
}

#[test]
fn a_dirty_clone_is_refused_before_it_moves() {
    let base = scratch();
    let (repo, first) = upstream(&base);
    let dir = base.path().join("clone");
    prepare(&origin(&repo, &first), &dir).unwrap();
    std::fs::write(dir.join("extra.c"), "int extra;\n").unwrap();
    let second = commit(&repo, "sub.c", "int sub(int a, int b) { return a - b; }\n");
    assert!(prepare(&origin(&repo, &second), &dir).is_err());
    assert_eq!(head(&dir), first, "HEAD stays where the edits were made");
}
