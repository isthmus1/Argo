//! Repository loading in a child process (this same binary, re-executed with the hidden
//! `--loader` flag; a flag rather than an environment variable, which could leak in by accident).
//!
//! Loading runs `git` and `cc` synchronously and can take long; a blocking thread can't be
//! cancelled, and abandoning it would leave those tools running after `argo-server` exits. In its
//! own process group the loader and everything it started are killed together when the load is
//! cancelled (a stop signal, the host dying) or fails.
//!
//! The child writes one frame to stdout: `O`, the origin's JSON length (u64 LE), the origin JSON
//! and the rows in BSATN (their SpacetimeDB encoding); or `E` and an error message.

use std::process::{ExitCode, Stdio};

use argo_db_client::CChunk;
use spacetimedb_sdk::__codegen::__lib::bsatn;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::args::Args;
use crate::group::{Group, isolate};
use crate::repo::{self, Loaded};

/// The loader process: load, write the frame, exit.
pub fn child(args: &Args) -> ExitCode {
    let frame = match repo::load(
        &args.repo(),
        args.flags(),
        args.allow_dirty,
        &args.checkout_dir(),
    )
    .and_then(encode)
    {
        Ok(frame) => frame,
        Err(e) => [b"E", e.as_bytes()].concat(),
    };
    match std::io::Write::write_all(&mut std::io::stdout().lock(), &frame) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// Run the loader on this process's own arguments and read its result. Dropping the future
/// kills the loader's group.
pub async fn load() -> Result<Loaded, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find argo-server: {e}"))?;
    let mut cmd = Command::new(exe);
    // First, so a `--` among the user's arguments can't make it positional.
    cmd.arg("--loader")
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = isolate(&mut cmd)
        .spawn()
        .map_err(|e| format!("cannot start the loader: {e}"))?;
    let group = Group::of(&child);
    let mut frame = Vec::new();
    let mut out = child.stdout.take().expect("piped stdout");
    let read = out.read_to_end(&mut frame).await;
    let status = child.wait().await;
    group.reaped();
    read.map_err(|e| format!("reading the loader: {e}"))?;
    let status = status.map_err(|e| format!("waiting for the loader: {e}"))?;
    match frame.split_first() {
        Some((b'O', body)) => decode(body),
        Some((b'E', msg)) => Err(String::from_utf8_lossy(msg).into_owned()),
        _ => Err(format!("the loader failed ({status})")),
    }
}

fn encode(loaded: Loaded) -> Result<Vec<u8>, String> {
    let origin = serde_json::to_vec(&loaded.origin).map_err(|e| e.to_string())?;
    let rows = bsatn::to_vec(&loaded.rows).map_err(|e| e.to_string())?;
    let len = (origin.len() as u64).to_le_bytes();
    Ok([&b"O"[..], &len, &origin, &rows].concat())
}

fn decode(body: &[u8]) -> Result<Loaded, String> {
    let bad = |e: &dyn std::fmt::Display| format!("unreadable loader output: {e}");
    let (len, rest) = body
        .split_first_chunk::<8>()
        .ok_or_else(|| bad(&"truncated"))?;
    let len = usize::try_from(u64::from_le_bytes(*len)).map_err(|e| bad(&e))?;
    if rest.len() < len {
        return Err(bad(&"truncated"));
    }
    let (origin, rows) = rest.split_at(len);
    Ok(Loaded {
        origin: serde_json::from_slice(origin).map_err(|e| bad(&e))?,
        rows: bsatn::from_slice::<Vec<CChunk>>(rows).map_err(|e| bad(&e))?,
    })
}

#[cfg(test)]
mod tests {
    use argo_db_client::{ChunkKind, Origin, Unit};

    use super::*;

    #[test]
    fn frames_roundtrip() {
        let origin = Origin {
            locator: "/r".into(),
            commit: Some("abc".into()),
            units: vec![Unit {
                path: "a.c".into(),
                skipped: None,
            }],
            flags: vec!["-I.".into()],
        };
        let row = CChunk {
            id: 7,
            node: Some(3),
            parent: None,
            name: "f".into(),
            kind: ChunkKind::Function,
            translatable: true,
            depth: 1,
            descendants: 0,
            path: "a.c".into(),
            line_start: 1,
            line_end: 2,
            ingested: "r.i".into(),
            byte_lo: 0,
            byte_hi: 4,
            source: "f(){}".into(),
            records: "{}".into(),
        };
        let frame = encode(Loaded {
            origin: origin.clone(),
            rows: vec![row.clone()],
        })
        .unwrap();
        let back = decode(&frame[1..]).unwrap();
        assert_eq!((back.origin, back.rows), (origin, vec![row]));
        assert!(decode(&frame[1..9]).is_err(), "truncated");
    }
}
