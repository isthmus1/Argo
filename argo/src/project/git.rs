//! Fetch a git reference into a fresh temporary directory.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A repository URL (or local path) plus an optional branch, tag or commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRef {
    pub url: String,
    pub rev: Option<String>,
}

impl GitRef {
    /// `url`, `url#rev` or `url@rev` (the last `@` after the host part, so `git@host:x` works).
    pub fn parse(spec: &str) -> GitRef {
        let spec = spec.trim();
        if let Some((url, rev)) = spec.rsplit_once('#') {
            return GitRef::new(url, Some(rev));
        }
        let host_end = spec.find("://").map_or(0, |i| i + 3);
        let after = &spec[host_end..];
        let path_start = after.find(['/', ':']).map_or(spec.len(), |i| host_end + i);
        match spec[path_start..].rfind('@') {
            Some(i) => GitRef::new(&spec[..path_start + i], Some(&spec[path_start + i + 1..])),
            None => GitRef::new(spec, None),
        }
    }

    pub fn new(url: &str, rev: Option<&str>) -> GitRef {
        let rev = rev
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string);
        GitRef {
            url: url.trim().to_string(),
            rev,
        }
    }

    /// The repository's name, for titles: `https://github.com/a/TinyEMU.git` → `TinyEMU`.
    pub fn name(&self) -> String {
        let tail = self
            .url
            .trim_end_matches('/')
            .rsplit(['/', ':'])
            .next()
            .unwrap_or(&self.url);
        let name = tail.strip_suffix(".git").unwrap_or(tail);
        if name.is_empty() {
            "project".into()
        } else {
            name.into()
        }
    }
}

/// Clone `r` into a new directory under the system temp dir and check out its revision.
/// Shallow when the server allows it; falls back to a full clone (e.g. a commit on a local repo).
pub fn fetch(r: &GitRef, log: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let dir =
        std::env::temp_dir().join(format!("argo-{}-{}-{stamp}", r.name(), std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let rev = r.rev.as_deref().unwrap_or("HEAD");
    log(&format!("Fetching {} @ {rev}", r.url));
    git(&dir, &["init", "-q"])?;
    git(&dir, &["remote", "add", "origin", &r.url])?;
    if git(&dir, &["fetch", "-q", "--depth", "1", "origin", rev]).is_err() {
        log("Shallow fetch refused; fetching full history");
        git(&dir, &["fetch", "-q", "--tags", "origin"])?;
        let target = if r.rev.is_some() {
            rev.to_string()
        } else {
            "origin/HEAD".into()
        };
        let target = if target == "origin/HEAD" {
            // No remote HEAD ref after a plain fetch: take the default branch it advertises.
            default_branch(&dir).unwrap_or(target)
        } else {
            target
        };
        git(&dir, &["checkout", "-q", "--detach", &target])?;
        return Ok(dir);
    }
    git(&dir, &["checkout", "-q", "--detach", "FETCH_HEAD"])?;
    Ok(dir)
}

fn default_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["ls-remote", "--symref", "origin", "HEAD"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.starts_with("ref:"))?;
    let branch = line
        .split_whitespace()
        .nth(1)?
        .strip_prefix("refs/heads/")?;
    Some(format!("origin/{branch}"))
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "git {}: {}",
            args[0],
            err.lines().last().unwrap_or("failed").trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse_with_and_without_revisions() {
        assert_eq!(
            GitRef::parse("https://github.com/a/b.git"),
            GitRef::new("https://github.com/a/b.git", None)
        );
        assert_eq!(
            GitRef::parse("https://github.com/a/b.git#v1.2"),
            GitRef::new("https://github.com/a/b.git", Some("v1.2"))
        );
        assert_eq!(
            GitRef::parse("https://github.com/a/b@main"),
            GitRef::new("https://github.com/a/b", Some("main"))
        );
        assert_eq!(
            GitRef::parse("git@github.com:a/b.git"),
            GitRef::new("git@github.com:a/b.git", None)
        );
        assert_eq!(
            GitRef::parse("git@github.com:a/b.git@dev"),
            GitRef::new("git@github.com:a/b.git", Some("dev"))
        );
        assert_eq!(
            GitRef::parse("https://user@host.example/r.git"),
            GitRef::new("https://user@host.example/r.git", None)
        );
        assert_eq!(
            GitRef::parse("https://github.com/a/TinyEMU.git").name(),
            "TinyEMU"
        );
    }
}
