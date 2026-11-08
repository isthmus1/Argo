//! The command line: which repository to serve and how to host it.

use std::path::{Path, PathBuf};

use argo_c::project::git::GitRef;
use clap::Parser;

/// Where `argo-db` lives in this checkout; the default module project to build and publish.
const MODULE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../argo-db");

/// Serve one C repository through SpacetimeDB for workers and the GUI.
#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// A local directory, or a git URL or path optionally followed by `#ref`.
    pub repo: String,
    /// Port for the local SpacetimeDB host. Not 3000, which a system host may already use.
    #[arg(long, default_value_t = 3100)]
    pub port: u16,
    /// SpacetimeDB data directory, kept across runs so a restart resumes instead of re-ingesting;
    /// it also holds a full checkout of the repository, which is parsed. Must be outside the
    /// repository (default: `$XDG_STATE_HOME/argo-server/<port>`, `~/.local/state/...` without
    /// it).
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
    /// Database name to publish the module under.
    #[arg(long, default_value = "argo")]
    pub module: String,
    /// Reset the database (`ingest_reset`: all chunks and Rust results) before ingesting: the
    /// explicit reset a different commit needs.
    #[arg(long)]
    pub fresh: bool,
    /// Drop the database when publishing; only for a module whose schema changed incompatibly.
    #[arg(long)]
    pub delete_data: bool,
    /// Ingest `HEAD` of a checkout with uncommitted changes, leaving the changes out.
    #[arg(long)]
    pub allow_dirty: bool,
    /// The `argo-db` module project to build and publish.
    #[arg(long, default_value = MODULE_PATH)]
    pub module_path: PathBuf,
    /// Preprocessor defines, `-DNAME` or `-DNAME=value`.
    #[arg(short = 'D', value_name = "NAME[=VALUE]")]
    pub defines: Vec<String>,
    /// Preprocessor include directories.
    #[arg(short = 'I', value_name = "DIR")]
    pub includes: Vec<PathBuf>,
    /// Run as the loader child process (see `loader`).
    #[arg(long, hide = true)]
    pub loader: bool,
}

/// Where the repository comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repo {
    /// An existing directory, opened in place.
    Dir(PathBuf),
    /// Anything else: fetched (a URL, or a path with `#ref`) into the checkout under the data dir.
    Git(GitRef),
}

impl Args {
    pub fn repo(&self) -> Repo {
        if Path::new(&self.repo).is_dir() {
            Repo::Dir(PathBuf::from(&self.repo))
        } else {
            Repo::Git(GitRef::parse(&self.repo))
        }
    }

    /// Extra preprocessor flags, in the form `argo::project::Options` takes.
    pub fn flags(&self) -> Vec<String> {
        let defines = self.defines.iter().map(|d| format!("-D{d}"));
        let includes = self.includes.iter().map(|i| format!("-I{}", i.display()));
        defines.chain(includes).collect()
    }

    /// A persistent per-user state directory by default: the system temp dir is often a
    /// RAM-backed tmpfs, too small for a large repository's checkout and lost on reboot.
    pub fn data_dir(&self) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| state_home().join("argo-server").join(self.port.to_string()))
    }

    /// Where a commit is checked out to be parsed, reused across runs.
    pub fn checkout_dir(&self) -> PathBuf {
        self.data_dir().join("checkout")
    }

    /// Refuse a data dir inside a directory repository: the host's files would make the tree look
    /// dirty, and a tree read in place would include the checkout's copy of itself.
    pub fn check_data_dir(&self) -> Result<(), String> {
        let Repo::Dir(repo) = self.repo() else {
            return Ok(());
        };
        let data = resolved(&self.data_dir());
        let repo = std::fs::canonicalize(&repo).map_err(|e| format!("{}: {e}", repo.display()))?;
        if data.starts_with(&repo) {
            return Err(format!(
                "the data dir {} is inside the repository {}; choose one outside it",
                data.display(),
                repo.display()
            ));
        }
        Ok(())
    }
}

/// `$XDG_STATE_HOME` (the XDG spec ignores a relative one), else `$HOME/.local/state`, else the
/// system temp dir.
fn state_home() -> PathBuf {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| Some(absolute(std::env::var_os("HOME")?)?.join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
}

/// `path` with its longest existing ancestor canonicalized, so a data dir that doesn't exist yet
/// still compares correctly (through symlinks and `..`).
fn resolved(path: &Path) -> PathBuf {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.into());
    abs.ancestors()
        .find_map(|a| {
            Some(
                std::fs::canonicalize(a)
                    .ok()?
                    .join(abs.strip_prefix(a).ok()?),
            )
        })
        .unwrap_or(abs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("argo-server").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn defaults() {
        let a = parse(&["https://example.com/r.git"]);
        assert_eq!((a.port, a.module.as_str(), a.fresh), (3100, "argo", false));
        assert!(!a.delete_data && !a.allow_dirty);
        assert!(
            a.module_path.join("Cargo.toml").is_file(),
            "argo-db in this checkout"
        );
        assert_eq!(a.data_dir(), state_home().join("argo-server/3100"));
        assert!(state_home().is_absolute());
        assert!(a.flags().is_empty());
        assert!(!a.loader, "hidden");
        assert_eq!(a.checkout_dir(), a.data_dir().join("checkout"));
    }

    #[test]
    fn the_data_dir_must_be_outside_a_directory_repository() {
        let dir = env!("CARGO_MANIFEST_DIR");
        let inside = format!("{dir}/src/not-yet");
        assert!(
            parse(&[dir, "--data-dir", &inside])
                .check_data_dir()
                .is_err()
        );
        assert!(parse(&[dir, "--data-dir", dir]).check_data_dir().is_err());
        let outside = format!("{dir}/../target/argo-server-data");
        assert!(
            parse(&[dir, "--data-dir", &outside])
                .check_data_dir()
                .is_ok()
        );
        let url = "https://example.com/r.git";
        assert!(parse(&[url, "--data-dir", "."]).check_data_dir().is_ok());
    }

    #[test]
    fn options_and_preprocessor_flags() {
        let a = parse(&[
            "repo",
            "--port",
            "3102",
            "--data-dir",
            "/tmp/d",
            "--module",
            "m",
            "--fresh",
            "--delete-data",
            "--allow-dirty",
            "-DFOO",
            "-D",
            "BAR=2",
            "-Iinc",
            "-I",
            "/abs",
        ]);
        assert_eq!((a.port, a.module.as_str(), a.fresh), (3102, "m", true));
        assert!(a.delete_data && a.allow_dirty);
        assert_eq!(a.data_dir(), PathBuf::from("/tmp/d"));
        assert_eq!(a.flags(), ["-DFOO", "-DBAR=2", "-Iinc", "-I/abs"]);
    }

    #[test]
    fn directories_open_in_place_and_the_rest_is_cloned() {
        let dir = env!("CARGO_MANIFEST_DIR");
        assert_eq!(parse(&[dir]).repo(), Repo::Dir(dir.into()));
        let at_ref = format!("{dir}#main");
        assert_eq!(
            parse(&[&at_ref]).repo(),
            Repo::Git(GitRef::new(dir, Some("main")))
        );
        assert_eq!(
            parse(&["https://example.com/r.git#v1"]).repo(),
            Repo::Git(GitRef::new("https://example.com/r.git", Some("v1")))
        );
    }
}
