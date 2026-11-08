//! The `spacetime` CLI steps: publishing the module and granting ownership.
//!
//! Both run as this machine's CLI identity. `init` records the publisher, and only it (or the
//! current owner) may `grant_ownership`, so the CLI is how the server's SDK identity, fresh on
//! every run, becomes the owner, also on a persisted database.

use std::path::Path;
use std::process::{Output, Stdio};

use spacetimedb_sdk::Identity;
use tokio::process::Command;

use crate::group::{Group, isolate};

/// Build and publish `module_path` as `db`. `delete_data` drops the existing database first,
/// only needed when the module's schema changed incompatibly; `--fresh` is the reducer-level
/// reset. Build and publish output goes to the server's stderr.
pub async fn publish(
    server: &str,
    module_path: &Path,
    db: &str,
    delete_data: bool,
) -> Result<(), String> {
    let mut cmd = spacetime(&["publish", "--yes", "--server", server, "--module-path"]);
    cmd.arg(module_path);
    if delete_data {
        cmd.arg("--delete-data=always");
    }
    cmd.arg(db).stdout(Stdio::from(std::io::stderr()));
    let out = run(cmd, "publish").await?;
    out.status
        .success()
        .then_some(())
        .ok_or_else(|| format!("spacetime publish failed ({})", out.status))
}

/// Make `owner` the owner of `db`.
pub async fn grant(server: &str, db: &str, owner: Identity) -> Result<(), String> {
    let arg = format!(r#"{{"__identity__": "0x{}"}}"#, owner.to_hex());
    let mut cmd = spacetime(&["call", "--server", server, db, "grant_ownership", &arg]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = run(cmd, "call").await?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(format!("grant_ownership failed: {}", err.trim()))
}

/// A `spacetime` command that never prompts.
fn spacetime(args: &[&str]) -> Command {
    let mut cmd = Command::new("spacetime");
    cmd.args(args).stdin(Stdio::null()).kill_on_drop(true);
    cmd
}

/// Run `cmd` in its own process group, so a cancelled step also stops what it started (the
/// cargo build behind `publish`).
async fn run(mut cmd: Command, what: &str) -> Result<Output, String> {
    let child = isolate(&mut cmd)
        .spawn()
        .map_err(|e| format!("cannot run spacetime {what}: {e}"))?;
    let group = Group::of(&child);
    let out = child.wait_with_output().await;
    group.reaped();
    out.map_err(|e| format!("spacetime {what}: {e}"))
}
