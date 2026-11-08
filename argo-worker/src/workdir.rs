//! Clone directories a worker holds exclusively for its lifetime, so no two workers on a host
//! ever share a checkout (each reads and validates against its own revision). The lock is an
//! advisory `flock` on the directory itself: it follows the inode, so a symlink or another
//! spelling of the same path is the same lock, and a later worker reuses a clean clone left by
//! an earlier one (`checkout::prepare`).
//!
//! The default directory is a reusable slot in the user's cache rather than a fresh tree per
//! process under the temp dir, which may be RAM-backed and is never cleaned up.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

/// A clone directory this process holds until it is dropped.
pub struct Held {
    pub dir: PathBuf,
    _lock: File,
}

/// Hold `dir` (created if missing); an error if another worker holds it.
pub fn hold(dir: &Path) -> Result<Held, String> {
    try_hold(dir)?.ok_or_else(|| format!("{} is in use by another worker", dir.display()))
}

/// The first free slot under `$XDG_CACHE_HOME` (else `~/.cache`)`/argo-worker/<module>`.
pub fn slot(module: &str) -> Result<Held, String> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache")))
        .ok_or("neither XDG_CACHE_HOME nor HOME is set; pass --workdir")?;
    // Database names are plain; anything else must not become a path component.
    let name: String = module
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    take(&cache.join("argo-worker").join(name))
}

fn take(base: &Path) -> Result<Held, String> {
    for n in 0u32.. {
        if let Some(held) = try_hold(&base.join(n.to_string()))? {
            return Ok(held);
        }
    }
    Err(format!("{}: no free slot", base.display()))
}

/// Lock `dir`, creating it first; `None` if another process holds it.
fn try_hold(dir: &Path) -> Result<Option<Held>, String> {
    let shown = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(shown)?;
    let lock = File::open(dir).map_err(shown)?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(Held {
            dir: dir.to_path_buf(),
            _lock: lock,
        })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(shown(e)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{hold, take};

    /// A released lock is free once no process holds its descriptor. A child process spawned
    /// at that moment (other tests run compilers) holds it until it execs, so allow a moment.
    fn eventually<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..200 {
            if let Some(t) = f() {
                return Some(t);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn concurrent_slots_differ_and_a_released_one_is_reused() {
        let base = tempfile::tempdir().unwrap();
        let first = take(base.path()).unwrap();
        let second = take(base.path()).unwrap();
        assert_ne!(first.dir, second.dir);
        let dir = first.dir.clone();
        drop(first);
        let reused = eventually(|| take(base.path()).ok().filter(|h| h.dir == dir));
        assert!(reused.is_some(), "slot {} is reused", dir.display());
    }

    #[test]
    fn an_explicit_directory_is_held_under_any_spelling() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("clone");
        let held = hold(&dir).unwrap();
        assert!(hold(&dir).is_err(), "the same path");
        let alias = base.path().join("alias");
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        assert!(hold(&alias).is_err(), "a symlink to it");
        assert!(
            hold(&base.path().join("clone/../clone")).is_err(),
            "another spelling"
        );
        drop(held);
        assert!(
            eventually(|| hold(&alias).ok()).is_some(),
            "released with its holder"
        );
    }

    #[test]
    fn a_slot_named_explicitly_is_still_exclusive() {
        let base = tempfile::tempdir().unwrap();
        let slot = take(base.path()).unwrap();
        assert!(hold(&slot.dir).is_err());
    }
}
