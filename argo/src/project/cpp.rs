//! The C preprocessor, gcc's: `gcc -E` treats `#include` as copy-paste and expands every macro, so
//! the parser only ever sees plain C. [`super::lines`] maps its output back to original files.
//!
//! Always gcc, never the host's `cc`: rucc reads gcc's dialect, and clang's output (macOS's `cc`)
//! carries clang-only spellings such as nullability qualifiers and availability attributes.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Always passed first, so a caller's flags can still override them.
///
/// `-std=gnu2x` is C23 with GNU extensions, the standard the parser reads
/// ([`crate::lexer::DEFAULT_STD`]); without it the output depends on the host compiler's default
/// (C23 from GCC 15, older before), and `bool` reaches the parser as a keyword on one host and
/// as `_Bool` on another. `gnu2x` rather than `gnu23` so GCC 9-13 accept it; GCC 14 and later
/// treat the two the same.
pub const DIALECT_FLAGS: [&str; 1] = ["-std=gnu2x"];

/// The preprocessor's driver.
pub const GCC: &str = "gcc";

/// Run `<cc> -E` on `input` (a file path, or C text on stdin when `stdin` is given).
pub fn preprocess(cc: &str, flags: &[String], input: Input<'_>) -> Result<Vec<u8>, Error> {
    let mut cmd = Command::new(cc);
    cmd.arg("-E").args(DIALECT_FLAGS).args(flags);
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
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::plain(format!("cannot run `{cc}`: {e}")))?;
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
        .map_err(|e| Error::plain(format!("`{cc}` failed: {e}")))?;
    if !out.status.success() {
        return Err(Error::from_stderr(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

#[derive(Clone, Copy)]
pub enum Input<'a> {
    File(&'a Path),
    Stdin(&'a str),
}

/// The preprocessor's first error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// What went wrong, without the location prefix: `#error MAX_XLEN must be defined`.
    pub message: String,
    /// The file (as the preprocessor spelled it) and line.
    pub at: Option<(String, u32)>,
}

impl Error {
    fn plain(message: String) -> Error {
        Error { message, at: None }
    }

    /// The first `error` line of gcc's or clang's diagnostics (`file:line:col: fatal error: msg`).
    pub fn from_stderr(err: &str) -> Error {
        let Some(line) = err
            .lines()
            .find(|l| l.contains("error"))
            .or_else(|| err.lines().find(|l| !l.trim().is_empty()))
        else {
            return Error::plain("preprocessor failed".into());
        };
        let line = line.trim();
        for marker in [": fatal error: ", ": error: "] {
            if let Some((loc, msg)) = line.split_once(marker) {
                // `loc` is `file:line:col` or `file:line`; file names may contain `:`.
                let mut parts = loc.rsplitn(3, ':');
                let (a, b, c) = (parts.next(), parts.next(), parts.next());
                let at = match (a, b, c) {
                    (Some(col), Some(n), Some(file)) if col.parse::<u32>().is_ok() => {
                        n.parse().ok().map(|n| (file.to_string(), n))
                    }
                    (Some(n), Some(file), rest) => n.parse().ok().map(|n| {
                        let file = rest.map_or(file.to_string(), |r| format!("{r}:{file}"));
                        (file, n)
                    }),
                    _ => None,
                };
                if at.is_some() {
                    return Error {
                        message: msg.trim().to_string(),
                        at,
                    };
                }
            }
        }
        Error::plain(line.to_string())
    }

    /// The header named by a missing-include error: gcc's `x.h: No such file or directory` or
    /// clang's `'x.h' file not found`.
    pub fn missing_header(&self) -> Option<&str> {
        let m = self.message.as_str();
        if let Some(h) = m.strip_suffix(": No such file or directory") {
            return Some(h.trim());
        }
        m.strip_suffix(" file not found")
            .map(|h| h.trim().trim_matches('\''))
    }

    /// `message (file:line)` with `short` applied to the file name.
    pub fn describe(&self, short: impl Fn(&str) -> String) -> String {
        match &self.at {
            Some((file, line)) => format!("{} ({}:{line})", self.message, short(file)),
            None => self.message.clone(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe(str::to_string))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcc_and_clang_errors_parse() {
        let gcc = Error::from_stderr(
            "In file included from x.c:1:\n/p/fs_wget.h:32:10: fatal error: openssl/aes.h: No such file or directory\n   32 | #include <openssl/aes.h>\ncompilation terminated.\n",
        );
        assert_eq!(gcc.at, Some(("/p/fs_wget.h".into(), 32)));
        assert_eq!(gcc.missing_header(), Some("openssl/aes.h"));
        assert_eq!(
            gcc.describe(|f| f.rsplit('/').next().unwrap().to_string()),
            "openssl/aes.h: No such file or directory (fs_wget.h:32)"
        );

        let clang = Error::from_stderr(
            "/Users/me/t/sdl.c:33:10: fatal error: 'SDL/SDL.h' file not found\n#include <SDL/SDL.h>\n1 error generated.\n",
        );
        assert_eq!(clang.missing_header(), Some("SDL/SDL.h"));
        assert_eq!(clang.at, Some(("/Users/me/t/sdl.c".into(), 33)));

        let e = Error::from_stderr("riscv_cpu.c:37:2: error: #error MAX_XLEN must be defined\n");
        assert_eq!(
            e.to_string(),
            "#error MAX_XLEN must be defined (riscv_cpu.c:37)"
        );
        assert_eq!(e.missing_header(), None);

        assert_eq!(
            Error::from_stderr("cc: unrecognized option\n").message,
            "cc: unrecognized option"
        );
    }
}
