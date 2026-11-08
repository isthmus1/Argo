//! The system C preprocessor: `cc -E` treats `#include` as copy-paste and expands every macro, so
//! the parser only ever sees plain C. [`super::lines`] maps its output back to original files.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Run `cc -E` on `input` (a file path, or C text on stdin when `stdin` is given).
pub fn preprocess(cc: &str, flags: &[String], input: Input<'_>) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(cc);
    cmd.arg("-E").args(flags);
    match input {
        Input::File(path) => {
            cmd.arg(path);
        }
        Input::Stdin(_) => {
            cmd.args(["-x", "c", "-"]);
        }
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot run `{cc}`: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    if let Input::Stdin(text) = input {
        // Written from a thread so a large output can't deadlock against a full stdin pipe.
        let text = text.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    } else {
        drop(stdin);
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("`{cc}` failed: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err
            .lines()
            .find(|l| l.contains("error"))
            .or_else(|| err.lines().next())
            .unwrap_or("preprocessor failed");
        return Err(first.trim().to_string());
    }
    Ok(out.stdout)
}

#[derive(Clone, Copy)]
pub enum Input<'a> {
    File(&'a Path),
    Stdin(&'a str),
}
