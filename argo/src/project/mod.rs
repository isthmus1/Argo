//! A whole C project as one protocol graph.
//!
//! Every `.c` file is a candidate translation unit. Each is first checked on its own (`cc -E`,
//! then the parser) so one file the parser can't read, or whose headers aren't installed, is
//! reported and left out instead of failing the project. The rest are combined into one input of
//! `#include "<file>.c"` lines and preprocessed once: includes are copy-paste and macros are
//! expanded, so the result is a single plain-C text, which the existing single-file pipeline reads.
//! Line markers map every top-level item back to its original file, which becomes its module;
//! code from outside the project (system headers) is dropped.

mod check;
pub mod cpp;
mod files;
pub mod git;
pub mod lines;

use std::path::{Path, PathBuf};

use argo_protocol::Snapshot;

use crate::{Parser, export, templates};
use cpp::Input;
use files::{relative, short};
use lines::Mapped;

pub use files::makefile_flags;

/// How to preprocess.
#[derive(Clone, Debug)]
pub struct Options {
    /// The C compiler driver used for `-E` (`cc` unless `$CC` says otherwise).
    pub cc: String,
    /// Extra preprocessor flags (`-DNAME=value`, `-Ipath`, `-UNAME`), after those read from the
    /// project's Makefile.
    pub flags: Vec<String>,
    /// Read `-D`/`-I`/`-U` flags from a top-level Makefile's CFLAGS/CPPFLAGS/DEFINES lines.
    pub makefile_flags: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cc: std::env::var("CC").unwrap_or_else(|_| "cc".into()),
            flags: vec![],
            makefile_flags: true,
        }
    }
}

/// A `.c` file and whether it made it into the graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// Path relative to the project root.
    pub path: String,
    /// Why it was left out, if it was.
    pub skipped: Option<String>,
}

/// Progress, for a UI or a log.
#[derive(Clone, Debug)]
pub enum Step {
    Fetching(String),
    Scanning,
    Checking { done: usize, total: usize },
    Combining { units: usize },
    Parsing,
    Building,
}

pub struct Opened {
    pub snapshot: Snapshot,
    pub units: Vec<Unit>,
    /// The preprocessor flags used, Makefile ones first.
    pub flags: Vec<String>,
    pub root: PathBuf,
}

/// Clone `r` into a temporary directory and open it.
pub fn open_git(
    r: &git::GitRef,
    opts: &Options,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    let dir = git::fetch(r, &|msg| progress(Step::Fetching(msg.to_string())))?;
    open(&dir, &r.name(), opts, progress)
}

/// Open the C project at `root` as one graph named `name`.
pub fn open(
    root: &Path,
    name: &str,
    opts: &Options,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    let root = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    progress(Step::Scanning);
    let sources = files::c_files(&root);
    if sources.is_empty() {
        return Err(format!("no .c files under {}", root.display()));
    }
    let mut flags = vec![format!("-I{}", root.display())];
    if opts.makefile_flags {
        flags.extend(makefile_flags(&root));
    }
    flags.extend(opts.flags.iter().cloned());

    let verdicts = check::all(&opts.cc, &flags, &sources, progress);
    let mut units: Vec<Unit> = sources
        .iter()
        .zip(verdicts)
        .map(|(p, skipped)| Unit {
            path: relative(&root, p),
            skipped,
        })
        .collect();

    // Combine the files that work alone; drop any that fail only when combined, and retry.
    loop {
        let included: Vec<&Path> = sources
            .iter()
            .zip(&units)
            .filter(|(_, u)| u.skipped.is_none())
            .map(|(p, _)| p.as_path())
            .collect();
        if included.is_empty() {
            return Err("no file could be preprocessed and parsed; see the skipped list".into());
        }
        progress(Step::Combining {
            units: included.len(),
        });
        let combined: String = included
            .iter()
            .map(|p| {
                format!(
                    "#include \"{}\"\n",
                    p.display()
                        .to_string()
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )
            })
            .collect();
        let raw = cpp::preprocess(&opts.cc, &flags, Input::Stdin(&combined))?;
        let mapped = Mapped::new(&raw);
        progress(Step::Parsing);
        let parsed = Parser::default().parse(&mapped.text);
        if let Some(d) = parsed.diagnostics.iter().find(|d| d.severity.is_fatal()) {
            let o = mapped.origin(d.span.lo);
            let culprit = o
                .unit
                .and_then(lines::canonical)
                .map(|u| relative(&root, &u));
            match culprit.and_then(|c| {
                units
                    .iter_mut()
                    .find(|u| u.path == c && u.skipped.is_none())
            }) {
                Some(u) => {
                    u.skipped = Some(format!(
                        "fails when combined: {} ({}:{})",
                        d.message,
                        short(o.file, &root),
                        o.line
                    ));
                    continue;
                }
                None => {
                    return Err(format!(
                        "combined parse failed at {}:{}: {}",
                        o.file, o.line, d.message
                    ));
                }
            }
        }
        progress(Step::Building);
        let graph = parsed
            .ast
            .build_graph(&parsed.source, templates::all())
            .map_err(|e| e.to_string())?;
        let module_of = |pos: u32| {
            let o = mapped.origin(pos);
            let path = lines::canonical(o.file)?;
            path.starts_with(&root).then(|| relative(&root, &path))
        };
        let (mut g, source) = export::to_protocol_modules(
            &graph,
            &parsed.ast,
            &format!("{name}.i"),
            name,
            &module_of,
        );
        g.freeze().map_err(|v| {
            let lines: Vec<_> = v.iter().take(8).map(ToString::to_string).collect();
            format!("graph failed freeze checks:\n{}", lines.join("\n"))
        })?;
        return Ok(Opened {
            snapshot: Snapshot::new(g, vec![source]),
            units,
            flags,
            root,
        });
    }
}
