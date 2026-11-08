//! A whole C project as one protocol graph.
//!
//! Every `.c` file is a candidate translation unit. Each is first checked on its own (`cc -E`,
//! then the parser) so one file the parser can't read, or whose headers aren't installed, is
//! reported and left out instead of failing the project. The rest are combined into one input of
//! `#include "<file>.c"` lines and preprocessed once: includes are copy-paste and macros are
//! expanded, so the result is a single plain-C text, which the existing single-file pipeline reads.
//! Line markers map every top-level item back to its original file, which becomes its module;
//! code from outside the project (system headers) is dropped.
//!
//! Flags come from the project's Makefile ([`make`]): shared ones, per-file ones from explicit
//! rules (emitted as `#define`/`#undef` around that file's `#include` in the combined input), and
//! optional feature switches a file is read without when they need a header that isn't installed.

mod check;
pub mod cpp;
mod dialect;
mod files;
pub mod git;
pub mod lines;
pub mod make;
mod places;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use argo_protocol::Snapshot;

use crate::{Parser, export, templates};
use cpp::Input;
use files::{relative, short};
use lines::Mapped;
pub use places::Places;

/// The flags every file gets from `root`'s Makefile.
pub fn makefile_flags(root: &Path) -> Vec<String> {
    make::read(root).flags
}

/// How to preprocess.
#[derive(Clone, Debug)]
pub struct Options {
    /// The C compiler driver used for `-E` (`cc` unless `$CC` says otherwise).
    pub cc: String,
    /// Extra preprocessor flags (`-DNAME=value`, `-Ipath`, `-UNAME`), after those read from the
    /// project's Makefile.
    pub flags: Vec<String>,
    /// Read flags from the project's top-level Makefile (see [`make`]).
    pub makefile_flags: bool,
    /// Set from another thread to stop a load: a running git command is killed, files not yet
    /// checked are skipped, and [`open`]/[`open_git`] return [`git::CANCELLED`].
    pub cancel: Arc<AtomicBool>,
}

impl Options {
    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(git::CANCELLED.into())
        } else {
            Ok(())
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            cc: std::env::var("CC").unwrap_or_else(|_| "cc".into()),
            flags: vec![],
            makefile_flags: true,
            cancel: Arc::default(),
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
    /// Flags for this file only, after the shared ones: `-DMAX_XLEN=128` from its own Makefile
    /// rule, or `-UCONFIG_FS_NET` when it was read without an optional feature whose header is
    /// missing.
    pub flags: Vec<String>,
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
    /// Where each byte of the snapshot's (combined, preprocessed) source came from.
    pub places: Places,
}

/// Clone `r` into a temporary directory and open it.
pub fn open_git(
    r: &git::GitRef,
    opts: &Options,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    let dir = git::fetch(
        r,
        &|msg| progress(Step::Fetching(msg.to_string())),
        &opts.cancel,
    )?;
    open(&dir, &r.name(), opts, progress)
}

/// Open the C project at `root` as one graph named `name`.
pub fn open(
    root: &Path,
    name: &str,
    opts: &Options,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Opened, String> {
    opts.cancelled()?;
    let root = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    progress(Step::Scanning);
    let sources = files::c_files(&root);
    if sources.is_empty() {
        return Err(format!("no .c files under {}", root.display()));
    }
    let makefile = if opts.makefile_flags {
        make::read(&root)
    } else {
        make::Makefile::default()
    };
    let mut flags = vec![format!("-I{}", root.display())];
    flags.extend(makefile.flags.iter().cloned());
    flags.extend(opts.flags.iter().cloned());
    // A caller's flag naming the same macro pins it: never turned off per file.
    let optional: Vec<String> = makefile
        .optional
        .iter()
        .filter(|d| !opts.flags.iter().any(|f| macro_name(f) == macro_name(d)))
        .cloned()
        .collect();

    let setup = check::Setup {
        cc: &opts.cc,
        root: &root,
        flags: &flags,
        optional: &optional,
        cancel: &opts.cancel,
    };
    let starts: Vec<(PathBuf, Vec<String>)> = sources
        .iter()
        .map(|p| (p.clone(), makefile.flags_for(p).to_vec()))
        .collect();
    let verdicts = check::all(&setup, &starts, progress);
    opts.cancelled()?;
    let mut units: Vec<Unit> = sources
        .iter()
        .zip(verdicts)
        .map(|(p, v)| Unit {
            path: relative(&root, p),
            skipped: v.skipped,
            flags: v.flags,
        })
        .collect();

    // Combine the files that work alone; drop any that fail only when combined, and retry.
    loop {
        opts.cancelled()?;
        let included: Vec<(&Path, &Unit)> = sources
            .iter()
            .zip(&units)
            .filter(|(_, u)| u.skipped.is_none())
            .map(|(p, u)| (p.as_path(), u))
            .collect();
        if included.is_empty() {
            return Err(nothing_read(&units));
        }
        progress(Step::Combining {
            units: included.len(),
        });
        let mut combined_flags = flags.clone();
        for (_, u) in &included {
            for f in u.flags.iter().filter(|f| f.starts_with("-I")) {
                if !combined_flags.contains(f) {
                    combined_flags.push(f.clone());
                }
            }
        }
        let combined = combined_input(&flags, &included);
        let raw =
            cpp::preprocess(&opts.cc, &combined_flags, Input::Stdin(&combined)).map_err(|e| {
                format!(
                    "preprocessing the combined files failed: {}",
                    e.describe(|f| short(f, &root))
                )
            })?;
        let mut mapped = Mapped::new(&raw);
        dialect::neutralize(&mut mapped.text);
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
                        short(o.file, &root),
                        o.line,
                        d.message
                    ));
                }
            }
        }
        opts.cancelled()?;
        let places = Places::new(mapped, &root);
        progress(Step::Building);
        let graph = parsed
            .ast
            .build_graph(&parsed.source, templates::all())
            .map_err(|e| e.to_string())?;
        let module_of = |pos: u32| places.at(pos).map(|(path, _)| path.to_string());
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
            places,
        });
    }
}

/// The error when every file was left out: a headline, then one `path: reason` line per file.
fn nothing_read(units: &[Unit]) -> String {
    let mut out = format!(
        "None of the {} .c files could be preprocessed and parsed.",
        units.len()
    );
    for u in units {
        out.push_str(&format!(
            "\n{}: {}",
            u.path,
            u.skipped.as_deref().unwrap_or("left out")
        ));
    }
    out
}

/// `-DNAME=value`, `-DNAME` or `-UNAME` → `NAME`.
fn macro_name(flag: &str) -> &str {
    flag.get(2..).unwrap_or("").split('=').next().unwrap_or("")
}

/// The `-D`/`-U` state the flags leave each macro in (later flags win).
fn defines(flags: &[String]) -> Vec<(&str, Option<&str>)> {
    let mut out: Vec<(&str, Option<&str>)> = vec![];
    for f in flags {
        let value = if let Some(d) = f.strip_prefix("-D") {
            Some(d.split_once('=').map_or("1", |(_, v)| v))
        } else if f.starts_with("-U") {
            None
        } else {
            continue;
        };
        let name = macro_name(f);
        out.retain(|(n, _)| *n != name);
        out.push((name, value));
    }
    out
}

/// One `#include` per file, preceded by the `#define`/`#undef` lines its own flags stand for and
/// followed by the lines restoring the shared state.
fn combined_input(flags: &[String], included: &[(&Path, &Unit)]) -> String {
    let shared = defines(flags);
    let set = |out: &mut String, name: &str, value: Option<&str>| {
        out.push_str(&format!("#undef {name}\n"));
        if let Some(v) = value {
            out.push_str(&format!("#define {name} {v}\n"));
        }
    };
    let quote = |p: &str| p.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = String::new();
    for (path, unit) in included {
        let own = defines(&unit.flags);
        for (name, value) in &own {
            set(&mut out, name, *value);
        }
        for f in &unit.flags {
            if let Some(h) = f.strip_prefix("-include") {
                out.push_str(&format!("#include \"{}\"\n", quote(h)));
            }
        }
        out.push_str(&format!(
            "#include \"{}\"\n",
            quote(&path.display().to_string())
        ));
        for (name, _) in &own {
            let restore = shared.iter().find(|(n, _)| n == name).and_then(|(_, v)| *v);
            set(&mut out, name, restore);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_file_flags_wrap_their_include() {
        let flags: Vec<String> = ["-I/p", "-DNET", "-DMAX=128", "-DMAX=64"]
            .map(String::from)
            .to_vec();
        let unit = |path: &str, f: &[&str]| Unit {
            path: path.into(),
            skipped: None,
            flags: f.iter().map(|s| s.to_string()).collect(),
        };
        let (a, b) = (unit("a.c", &["-DXLEN=128", "-UNET"]), unit("b.c", &[]));
        let text = combined_input(
            &flags,
            &[(Path::new("/p/a.c"), &a), (Path::new("/p/b.c"), &b)],
        );
        assert_eq!(
            text,
            "#undef XLEN\n#define XLEN 128\n#undef NET\n#include \"/p/a.c\"\n#undef XLEN\n#undef NET\n#define NET 1\n#include \"/p/b.c\"\n"
        );
        assert_eq!(defines(&flags), [("NET", Some("1")), ("MAX", Some("64"))]);
    }

    #[test]
    fn a_cancelled_load_stops_between_files() {
        let dir = std::env::temp_dir().join(format!("argo-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            std::fs::write(dir.join(format!("f{i}.c")), format!("int f{i}(void) {{ return {i}; }}\n")).unwrap();
        }
        let opts = Options::default();
        opts.cancel.store(true, Ordering::Relaxed);
        assert_eq!(open(&dir, "c", &opts, &|_| {}).err().as_deref(), Some(git::CANCELLED));

        // Cancelled while checking: the files after it aren't read and nothing is combined.
        let opts = Options::default();
        let combined = AtomicBool::new(false);
        let r = open(&dir, "c", &opts, &|step| match step {
            Step::Checking { .. } => opts.cancel.store(true, Ordering::Relaxed),
            Step::Combining { .. } => combined.store(true, Ordering::Relaxed),
            _ => {}
        });
        assert_eq!(r.err().as_deref(), Some(git::CANCELLED));
        assert!(!combined.load(Ordering::Relaxed));
    }

    #[test]
    fn nothing_read_lists_every_file_with_its_reason() {
        let units = [
            Unit {
                path: "riscv_cpu.c".into(),
                skipped: Some("#error MAX_XLEN must be defined (riscv_cpu.c:37)".into()),
                flags: vec![],
            },
            Unit {
                path: "sdl.c".into(),
                skipped: Some("SDL/SDL.h: No such file or directory (sdl.c:33)".into()),
                flags: vec![],
            },
        ];
        assert_eq!(
            nothing_read(&units),
            "None of the 2 .c files could be preprocessed and parsed.\nriscv_cpu.c: #error MAX_XLEN must be defined (riscv_cpu.c:37)\nsdl.c: SDL/SDL.h: No such file or directory (sdl.c:33)"
        );
    }
}
