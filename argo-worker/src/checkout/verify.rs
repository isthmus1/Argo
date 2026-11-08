//! Whether a clone is exactly the recorded revision, checked before it is moved and on every
//! reuse.

use std::path::Path;

use super::git::git;

/// Refuse `dir` unless it is exactly `commit`: HEAD there and nothing modified, staged,
/// untracked or ignored.
pub fn exact(dir: &Path, commit: &str) -> Result<(), String> {
    let id = |rev: &str| git(Some(dir), ["rev-parse", "--verify", "--quiet", rev]);
    let (head, want) = (id("HEAD")?, id(&format!("{commit}^{{commit}}"))?);
    if head != want {
        return Err(format!(
            "{} moved to {}, not {commit}; pass another --workdir",
            dir.display(),
            head.trim()
        ));
    }
    clean(dir, commit)
}

/// Refuse local changes rather than discard them. Ignored files count: a build output or a
/// generated header in the clone would reach prompts and builds as if it were the revision.
pub fn clean(dir: &Path, commit: &str) -> Result<(), String> {
    let status = [
        "status",
        "--porcelain",
        "--untracked-files=all",
        "--ignored",
    ];
    let status = git(Some(dir), status)?;
    if status.trim().is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} has local changes, so it is not {commit}; clean it or pass another --workdir:\n{}",
        dir.display(),
        status.trim_end()
    ))
}
