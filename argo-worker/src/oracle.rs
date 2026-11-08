//! The project's own build and tests as the last check of a piece (`--test-cmd`). Each worker
//! keeps a copy of its clone beside it (`<clone>.build`), made once, so the project's build
//! stays incremental across claims. The candidate's spliced unit (preprocessed, with the C
//! replacement in place) is written over the piece's file there, the command runs in that
//! directory, and the file is put back whatever happens. Exit 0 passes.
//!
//! The command gets `ARGO_RUST_LIB` (the candidate's Rust as a static library, to add to the
//! link, e.g. `make LIBS="$ARGO_RUST_LIB $ARGO_RUST_LIBS -lm"`), `ARGO_RUST_LIBS` (the native
//! libraries that library needs), `ARGO_UNIT` (the piece's file), `ARGO_BUILD` (the copy) and
//! `ARGO_SYMBOL` (the piece's exported function). Exit 77, automake's "skipped", says the
//! project's tests don't cover the piece (e.g. `nm temu | rg -q "$ARGO_SYMBOL" || exit 77`):
//! the piece isn't rejected, but isn't counted as tested either.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::program::Site;

/// What the project's tests said about a piece.
#[derive(Debug, PartialEq, Eq)]
pub enum Tested {
    Passed,
    /// Exit 77: the piece isn't in what the tests run.
    NotCovered,
}

/// Run `cmd` with `unit` (the spliced unit's text) in place of the piece's file.
/// `Err((summary, full log))` when the command fails or can't run.
pub fn run(
    cmd: &str,
    symbol: &str,
    site: &Site,
    unit: &str,
    lib: &Path,
    libs: &str,
    dir: &Path,
) -> Result<Tested, (String, String)> {
    let build = build_dir(&site.root).map_err(|e| ("no build copy of the clone".into(), e))?;
    let file = build.join(&site.path);
    let original = std::fs::read(&file).map_err(|e| (format!("{}: {e}", file.display()), String::new()))?;
    std::fs::write(&file, unit).map_err(|e| (format!("{}: {e}", file.display()), String::new()))?;
    let lib = std::path::absolute(lib).unwrap_or_else(|_| lib.to_path_buf());
    let out = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(&build)
        .env("ARGO_RUST_LIB", &lib)
        .env("ARGO_RUST_LIBS", libs)
        .env("ARGO_UNIT", &site.path)
        .env("ARGO_BUILD", &build)
        .env("ARGO_SYMBOL", symbol)
        .output();
    // Put the file back even when the command didn't run; its newer mtime makes the next build
    // recompile it.
    let _ = std::fs::write(&file, original);
    let out = out.map_err(|e| (format!("`{cmd}` didn't run: {e}"), String::new()))?;
    let log = format!(
        "$ {cmd}\n(in {}, with {} spliced in)\n{}{}",
        build.display(),
        site.path,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::write(dir.join("test.log"), &log);
    if out.status.success() {
        Ok(Tested::Passed)
    } else if out.status.code() == Some(77) {
        Ok(Tested::NotCovered)
    } else {
        let summary = format!(
            "the project's build and tests fail with this piece in it ({}; full log at {})",
            out.status,
            dir.join("test.log").display()
        );
        Err((summary, log))
    }
}

/// `<clone>.build`, copied from the clone the first time.
fn build_dir(root: &Path) -> Result<PathBuf, String> {
    let mut name = root.file_name().unwrap_or_default().to_os_string();
    name.push(".build");
    let build = root.with_file_name(name);
    if !build.exists() {
        // A unique partial copy, renamed into place once whole; a crashed copy is left aside.
        let tmp = build.with_extension(format!("build-partial-{}", std::process::id()));
        let out = Command::new("cp")
            .arg("-a")
            .arg(root)
            .arg(&tmp)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        std::fs::rename(&tmp, &build).map_err(|e| e.to_string())?;
    }
    Ok(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_sees_the_spliced_unit_and_the_file_is_put_back() {
        let tmp = std::env::temp_dir().join(format!("argo-oracle-{}-{:?}", std::process::id(), std::time::SystemTime::now()));
        let root = tmp.join("clone");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.c"), "original").unwrap();
        let site = Site { path: "a.c".into(), root: root.clone(), ..Site::default() };
        let lib = tmp.join("lib.a");
        let cmd = "rg -q spliced \"$ARGO_UNIT\" && test -n \"$ARGO_RUST_LIB\" && test \"$ARGO_SYMBOL\" = f__part1";
        assert_eq!(run(cmd, "f__part1", &site, "spliced", &lib, "", &tmp), Ok(Tested::Passed));
        assert_eq!(run("exit 77", "f__part1", &site, "spliced", &lib, "", &tmp), Ok(Tested::NotCovered));
        assert_eq!(std::fs::read_to_string(tmp.join("clone.build/a.c")).unwrap(), "original");
        let failed = run("exit 3", "f__part1", &site, "spliced", &lib, "", &tmp).unwrap_err();
        assert!(failed.0.contains("fail"), "{failed:?}");
        assert_eq!(std::fs::read_to_string(tmp.join("clone.build/a.c")).unwrap(), "original");
    }
}
