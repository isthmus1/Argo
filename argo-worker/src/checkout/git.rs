//! The git CLI, with its failures as readable errors.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// Run git (in `dir`, if given); its stdout on success.
pub fn git<S: AsRef<OsStr>>(
    dir: Option<&Path>,
    args: impl IntoIterator<Item = S>,
) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    let args: Vec<S> = args.into_iter().collect();
    let out = cmd.args(&args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let shown: Vec<_> = args.iter().map(|a| a.as_ref().to_string_lossy()).collect();
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {}: {}", shown.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
