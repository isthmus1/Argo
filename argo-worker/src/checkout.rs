//! The worker's own clone of the repository at the exact commit the server ingested, so the
//! context it reads (and, later, what the validator builds) is that revision and nothing else.
//! The clone is read-only once prepared: every reuse re-checks its HEAD and that nothing in it
//! changed, ignored files included, so a validator must build in a directory of its own.

mod git;
mod verify;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use argo_db_client::Origin;

use crate::workdir::Held;
use git::git;
use verify::{clean, exact};

/// A clone directory this worker holds, and the revision it holds. [`Checkout::sync`] runs
/// before every claim is worked, so an owner's reset and re-ingest of another commit moves the
/// clone with it.
pub struct Checkout {
    dir: Held,
    /// The origin last prepared and what that gave.
    at: Option<(Origin, Option<PathBuf>)>,
}

impl Checkout {
    /// Only the holder of a directory may clone into it, so two workers never share one.
    pub fn new(dir: Held) -> Checkout {
        Checkout { dir, at: None }
    }

    /// The clone at `origin`'s commit: prepared when the origin changed since last time,
    /// otherwise re-checked, since anything may have written to it in between.
    pub fn sync(&mut self, origin: &Origin) -> Result<Option<PathBuf>, String> {
        let same =
            |(at, _): &(Origin, _)| (&at.locator, &at.commit) == (&origin.locator, &origin.commit);
        match &self.at {
            Some(at @ (_, path)) if same(at) => {
                if let (Some(dir), Some(commit)) = (path, &origin.commit) {
                    exact(dir, commit)?;
                }
                Ok(path.clone())
            }
            _ => {
                let path = prepare(origin, &self.dir.dir)?;
                self.at = Some((origin.clone(), path.clone()));
                Ok(path)
            }
        }
    }
}

/// Clone `origin` into `dir` (or reuse a clone already there) and check out its commit,
/// detached, refusing a clone with local changes before moving it. `None` when the ingest
/// recorded no commit: a plain directory can't be reproduced, so the worker prompts from the
/// database's C text alone rather than guess a revision.
pub fn prepare(origin: &Origin, dir: &Path) -> Result<Option<PathBuf>, String> {
    let Some(commit) = &origin.commit else {
        eprintln!(
            "argo-worker: {} was ingested without a commit; prompting without repository context",
            origin.locator
        );
        return Ok(None);
    };
    // The server's `url#ref` names how it found the commit; the commit alone pins the revision.
    let url = origin
        .locator
        .split_once('#')
        .map_or(&*origin.locator, |(u, _)| u);
    // Both reach git as positional arguments; neither may read as an option.
    if url.starts_with('-') || commit.is_empty() || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("refusing origin {url:?} at {commit:?}"));
    }
    if !dir.join(".git").exists() {
        let clone = ["clone", "--quiet", "--no-checkout", "--"].map(OsStr::new);
        git(
            None,
            clone.into_iter().chain([url.as_ref(), dir.as_os_str()]),
        )?;
    } else if dir.join(".git/index").exists() {
        // A reused clone: `checkout` would carry local edits along, so refuse them before
        // moving HEAD. Without an index the first checkout never ran; there is nothing to keep.
        clean(dir, commit)?;
    }
    let checkout = [
        "-c",
        "advice.detachedHead=false",
        "checkout",
        "--quiet",
        "--detach",
    ];
    let checkout = checkout.into_iter().chain([commit.as_str()]);
    if git(Some(dir), checkout.clone()).is_err() {
        // Not reachable from the cloned branches (or a reused, older clone, possibly of another
        // locator): fetch it by id from the recorded locator.
        git(Some(dir), ["fetch", "--quiet", url, commit])?;
        git(Some(dir), checkout)?;
    }
    exact(dir, commit)?;
    Ok(Some(dir.to_path_buf()))
}

#[cfg(test)]
mod tests;
