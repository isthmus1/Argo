//! The one checkout the server parses, kept under the data directory and reused across runs.
//!
//! Parsing a working tree in place can't promise a commit: `project::open` reads every `.c` file
//! and every header it includes, ignored ones too (generated sources, a local `config.h`), and
//! `git status` doesn't show those. A checkout of the commit holds exactly what a worker's clone
//! holds. It lives in the data dir rather than a fresh temporary directory (as
//! `argo::project::git::fetch` would make) so restarts reuse it and nothing piles up.

use std::path::{Path, PathBuf};
use std::process::Command;

use argo_c::project::git::GitRef;

/// Check `r` out at `dir`: fetched (shallow when the source allows it) and detached, refusing a
/// directory holding anything the commit doesn't.
pub fn at(r: &GitRef, dir: &Path, log: &dyn Fn(&str)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q"])?;
    }
    let rev = r.rev.as_deref().unwrap_or("HEAD");
    log(&format!(
        "Fetching {} @ {rev} into {}",
        r.url,
        dir.display()
    ));
    let target = if git(dir, &["fetch", "-q", "--depth", "1", &r.url, rev]).is_ok() {
        "FETCH_HEAD"
    } else if r.rev.is_none() {
        log("Shallow fetch refused; fetching full history");
        git(dir, &["fetch", "-q", &r.url])?;
        "FETCH_HEAD"
    } else {
        // A commit the source won't hand out by name: fetch everything, then find it here.
        log("Shallow fetch refused; fetching full history");
        let heads = "+refs/heads/*:refs/remotes/origin/*";
        git(dir, &["fetch", "-q", "--tags", &r.url, heads])?;
        rev
    };
    git(dir, &["checkout", "-q", "-f", "--detach", target])?;
    let stray = git(
        dir,
        &[
            "status",
            "--porcelain",
            "--ignored",
            "--untracked-files=all",
        ],
    )?;
    if !stray.is_empty() {
        return Err(format!(
            "{} holds files outside the commit ({}); move it away",
            dir.display(),
            stray.lines().next().unwrap_or_default().trim()
        ));
    }
    std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// A git command's stdout, or its last error line.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(format!(
        "git {}: {}",
        args[0],
        err.lines().last().unwrap_or("failed").trim()
    ))
}
