//! The repository as `c_chunk` rows: opened with the C front end, recorded with the locator and
//! exact commit a worker clones.
//!
//! A recorded commit is parsed from a checkout of exactly that commit ([`checkout`]), never from
//! a working tree, so ignored files the worker's clone lacks can't slip in. The rows depend only
//! on the repository at that commit, never on where it was checked out, so a restart reproduces
//! the sealed rows exactly and is a no-op: paths under the checkout are recorded relative to it,
//! and `__FILE__` expands relative to it too.

use std::path::Path;

use argo_c::project::{self, Options, Step, git::GitRef};
use argo_db_client::{CChunk, Origin, Unit, to_rows};

use crate::args::Repo;
use crate::{checkout, commit, flags};

/// Everything one ingest sends.
pub struct Loaded {
    pub origin: Origin,
    pub rows: Vec<CChunk>,
}

/// Open `repo` and convert it; a commit is checked out at `workdir` first. Blocking: fetching,
/// preprocessing and parsing run here, which is why only the loader process calls it.
pub fn load(
    repo: &Repo,
    mut flags: Vec<String>,
    allow_dirty: bool,
    workdir: &Path,
) -> Result<Loaded, String> {
    let fetch = |r: &GitRef| checkout::at(r, workdir, &|msg| progress(Step::Fetching(msg.into())));
    let (root, name, locator, commit) = match repo {
        Repo::Dir(dir) => {
            let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = dir
                .file_name()
                .map_or("project".into(), |n| n.to_string_lossy().into_owned());
            let locator = dir.display().to_string();
            match commit::resolve(&dir, allow_dirty)? {
                Some(c) => {
                    let root = fetch(&GitRef::new(&locator, Some(&c)))?;
                    flags = rerooted(flags, &dir, &root);
                    (root, name, locator, Some(c))
                }
                // Nothing a worker could clone: read the tree as it is.
                None => (dir, name, locator, None),
            }
        }
        Repo::Git(r) => {
            // A local repository is named by its absolute path: `git fetch` runs inside the
            // checkout, where a relative path means something else, and a worker started anywhere
            // must be able to clone it. URLs stay as given.
            let local = std::fs::canonicalize(&r.url).ok();
            let url = local
                .as_ref()
                .map_or_else(|| r.url.clone(), |p| p.display().to_string());
            let root = fetch(&GitRef::new(&url, r.rev.as_deref()))?;
            if let Some(src) = &local {
                // The source's work tree (not its `.git`) is where `-I` paths would point.
                let tree = if src.ends_with(".git") {
                    src.parent().unwrap_or(src)
                } else {
                    src
                };
                flags = rerooted(flags, tree, &root);
            }
            let commit = commit::resolve(&root, allow_dirty)?;
            (root, r.name(), url, commit)
        }
    };
    let prefix_map = format!("-fmacro-prefix-map={}/=", root.display());
    flags.push(prefix_map.clone());
    let opts = Options {
        flags,
        ..Options::default()
    };
    let opened = project::open(&root, &name, &opts, &progress)?;
    for u in &opened.units {
        if let Some(why) = &u.skipped {
            eprintln!("argo-server: skipped {}: {why}", u.path);
        }
    }
    let origin = Origin {
        locator,
        commit,
        units: opened
            .units
            .iter()
            .map(|u| Unit {
                path: u.path.clone(),
                skipped: u.skipped.as_deref().map(|s| flags::unrooted(s, &root)),
            })
            .collect(),
        // The prefix map only names this run's checkout; it isn't a property of the project.
        flags: opened
            .flags
            .iter()
            .filter(|f| **f != prefix_map)
            .map(|f| flags::unrooted_flag(f, &root))
            .collect(),
    };
    let rows = to_rows(&opened.snapshot, &origin, &|file, pos| {
        let (path, line) = opened.places.at(file, pos)?;
        Some((path.to_string(), line))
    })
    .map_err(|e| e.to_string())?;
    Ok(Loaded { origin, rows })
}

/// Include directories the user gave inside the source tree `from`, taken from the checkout `to`
/// instead, so they come from the recorded commit rather than whatever the tree holds now.
fn rerooted(flags: Vec<String>, from: &Path, to: &Path) -> Vec<String> {
    flags.iter().map(|f| flags::rerooted(f, from, to)).collect()
}

fn progress(step: Step) {
    match step {
        Step::Fetching(msg) => eprintln!("argo-server: {msg}"),
        Step::Checking { done, total } if done < total => {}
        step => eprintln!("argo-server: {step:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    /// A committed repository with an include directory, a file the preprocessor rejects (so a
    /// skip message naming its path is recorded), a function returning `__FILE__`, and an ignored
    /// C file present in the working tree.
    fn committed_repo(name: &str) -> (PathBuf, String) {
        let src = crate::scratch::dir(name);
        std::fs::create_dir_all(src.join("include")).unwrap();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/compute/compute.c");
        std::fs::copy(fixture, src.join("compute.c")).unwrap();
        std::fs::write(src.join("include/extra.h"), "int extra(int);\n").unwrap();
        std::fs::write(src.join("broken.c"), "#include \"missing.h\"\n").unwrap();
        std::fs::write(
            src.join("where.c"),
            "const char *where(void) { return __FILE__; }\n",
        )
        .unwrap();
        std::fs::write(src.join(".gitignore"), "generated.c\n").unwrap();
        git(&src, &["init", "-q"]);
        git(&src, &["add", "."]);
        git(&src, &["commit", "-qm", "x"]);
        std::fs::write(
            src.join("generated.c"),
            "int generated(void) { return 1; }\n",
        )
        .unwrap();
        let head = git(&src, &["rev-parse", "HEAD"]);
        (src, head)
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// The restart regression: checkouts of one commit in different directories must give
    /// identical rows (paths and `__FILE__` included), or the sealed database rejects the second
    /// ingest.
    #[test]
    fn checkouts_of_one_commit_give_identical_rows() {
        let (src, head) = committed_repo("clone-src");
        let r = Repo::Git(GitRef::new(src.to_str().unwrap(), Some(&head)));
        let flags = vec![format!("-I{}", src.join("include").display())];
        let a = load(&r, flags.clone(), false, &crate::scratch::dir("a")).unwrap();
        let b = load(&r, flags, false, &crate::scratch::dir("b")).unwrap();
        assert_eq!(a.origin.commit.as_deref(), Some(head.as_str()));
        assert!(a.origin.flags.contains(&"-I.".to_string()));
        assert!(a.origin.units.iter().any(|u| u.skipped.is_some()));
        assert!(a.rows.iter().any(|r| r.source.contains("\"where.c\"")));
        assert_eq!(a.origin, b.origin);
        assert_eq!(a.rows, b.rows);
    }

    /// A clean committed directory is parsed from its commit: an ignored C file in the working
    /// tree is not ingested, and the workdir is reused on a second load.
    #[test]
    fn a_committed_directory_ingests_only_its_commit() {
        let (src, head) = committed_repo("dir-src");
        let workdir = crate::scratch::dir("workdir");
        let r = Repo::Dir(src.clone());
        let flags = vec![format!("-I{}", src.join("include").display())];
        for _ in 0..2 {
            let loaded = load(&r, flags.clone(), false, &workdir).unwrap();
            assert_eq!(loaded.origin.commit.as_deref(), Some(head.as_str()));
            assert_eq!(loaded.origin.locator, src.display().to_string());
            assert!(loaded.origin.units.iter().all(|u| u.path != "generated.c"));
            assert!(loaded.rows.iter().all(|r| r.name != "generated"));
            assert!(loaded.origin.flags.contains(&"-Iinclude".to_string()));
        }
    }

    /// A local `path#commit` takes the headers it includes from that commit too, not from the
    /// source's work tree, which has moved on.
    #[test]
    fn a_local_reference_includes_headers_from_its_commit() {
        let src = crate::scratch::dir("header-src");
        std::fs::create_dir_all(src.join("include")).unwrap();
        std::fs::write(
            src.join("value.c"),
            "#include \"value.h\"\nint value(void) { return VALUE; }\n",
        )
        .unwrap();
        let commit = |value: &str| {
            std::fs::write(src.join("include/value.h"), value).unwrap();
            git(&src, &["add", "."]);
            git(&src, &["commit", "-qm", "x"]);
            git(&src, &["rev-parse", "HEAD"])
        };
        git(&src, &["init", "-q"]);
        let first = commit("#define VALUE 11\n");
        commit("#define VALUE 22\n");
        let r = Repo::Git(GitRef::new(src.to_str().unwrap(), Some(&first)));
        let flags = vec![format!("-I{}", src.join("include").display())];
        let loaded = load(&r, flags, false, &crate::scratch::dir("header-co")).unwrap();
        assert_eq!(loaded.origin.commit.as_deref(), Some(first.as_str()));
        let value = loaded.rows.iter().find(|r| r.name == "value").unwrap();
        assert!(value.source.contains("return 11;"), "{}", value.source);
        assert!(loaded.origin.flags.contains(&"-Iinclude".to_string()));
    }

    /// A relative local path with `#ref` is resolved against the working directory, not the
    /// checkout `git fetch` runs in, and recorded absolute.
    #[test]
    fn a_relative_local_reference_is_resolved_and_recorded_absolute() {
        let (src, head) = committed_repo("relative-src");
        let cwd = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        let common = cwd.ancestors().find(|a| src.starts_with(a)).unwrap();
        let ups = cwd.strip_prefix(common).unwrap().components().count();
        let rel: PathBuf = std::iter::repeat_n(Path::new(".."), ups)
            .collect::<PathBuf>()
            .join(src.strip_prefix(common).unwrap());
        assert!(rel.is_relative());
        let r = Repo::Git(GitRef::new(rel.to_str().unwrap(), Some("HEAD")));
        let loaded = load(&r, vec![], false, &crate::scratch::dir("relative-co")).unwrap();
        assert_eq!(loaded.origin.locator, src.display().to_string());
        assert_eq!(loaded.origin.commit.as_deref(), Some(head.as_str()));
    }
}
