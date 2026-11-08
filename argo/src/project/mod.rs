//! A whole C project as one protocol graph.
//!
//! Every `.c` file is its own translation unit, as a compiler sees it: preprocessed (`gcc -E`, so
//! includes are pasted in and macros expanded), parsed and built on its own, in parallel. A file
//! the parser can't read, or whose headers aren't installed, is reported and left out instead of
//! failing the project. Files are never pasted together: a macro, typedef or `static` name one
//! file defines means nothing to another, and combining them would let it.
//!
//! The units are then merged into one graph. Line markers map every top-level item back to its
//! original file, which becomes its module; code from outside the project (system headers) is
//! dropped. A project header's items come in with every unit that includes it and are kept once,
//! unless it expands differently in another unit, which keeps that version too. Each unit is
//! recorded as one source file holding only the items it contributed, laid end to end, so system
//! headers aren't stored once per unit; [`Places`] traces its bytes back to the original lines.
//!
//! Flags come from the project's Makefile ([`make`]): shared ones, per-file ones from explicit
//! rules, and optional feature switches a file is read without when they need a header that isn't
//! installed.

mod check;
pub mod cpp;
mod files;
pub mod git;
pub mod lines;
pub mod make;
mod places;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use argo_protocol::{FileId, Snapshot, SourceFile};

use crate::export::{Placement, ProgramBuilder};
use files::relative;
pub use places::Places;

/// The flags every file gets from `root`'s Makefile.
pub fn makefile_flags(root: &Path) -> Vec<String> {
    make::read(root).flags
}

/// How to preprocess.
#[derive(Clone, Debug)]
pub struct Options {
    /// The C compiler driver used for `-E`: [`cpp::GCC`]. rucc follows gcc's dialect, so a clang
    /// preprocessor would hand the parser clang-only spellings.
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
            cc: cpp::GCC.into(),
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
    /// Files preprocessed, parsed and built so far, each on its own.
    Checking {
        done: usize,
        total: usize,
    },
    /// Every file is read; the merged graph is being checked.
    Merging {
        units: usize,
    },
}

pub struct Opened {
    /// One source per unit that contributed items: its preprocessed items, laid end to end.
    pub snapshot: Snapshot,
    pub units: Vec<Unit>,
    /// The preprocessor flags used, Makefile ones first.
    pub flags: Vec<String>,
    pub root: PathBuf,
    /// Where each byte of the snapshot's (preprocessed) sources came from.
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
    let mut merge = Merge::new(&root, name);
    let verdicts = check::all(&setup, &starts, progress, &mut |i, read| {
        merge.add(&relative(&root, &sources[i]), read);
    });
    opts.cancelled()?;
    let units: Vec<Unit> = sources
        .iter()
        .zip(verdicts)
        .map(|(p, v)| Unit {
            path: relative(&root, p),
            skipped: v.skipped,
            flags: v.flags,
        })
        .collect();
    let read = units.iter().filter(|u| u.skipped.is_none()).count();
    if read == 0 {
        return Err(nothing_read(&units));
    }
    progress(Step::Merging { units: read });
    let Merge {
        program,
        sources,
        places,
        ..
    } = merge;
    let mut g = program.finish();
    g.freeze().map_err(|v| {
        let lines: Vec<_> = v.iter().take(8).map(ToString::to_string).collect();
        format!("graph failed freeze checks:\n{}", lines.join("\n"))
    })?;
    Ok(Opened {
        snapshot: Snapshot::new(g, sources),
        units,
        flags,
        root,
        places,
    })
}

/// The units merged so far.
struct Merge<'a> {
    root: &'a Path,
    program: ProgramBuilder,
    sources: Vec<SourceFile>,
    places: Places,
    /// Preprocessor file names resolved to a project path (index into `places`), or `None` for
    /// files outside the project.
    resolved: HashMap<String, Option<u32>>,
    /// Top-level items already taken, by original file, line and exact preprocessed text.
    seen: HashSet<(u32, u32, Vec<u8>)>,
}

impl<'a> Merge<'a> {
    fn new(root: &'a Path, name: &str) -> Self {
        Merge {
            root,
            program: ProgramBuilder::new(name),
            sources: vec![],
            places: Places::default(),
            resolved: HashMap::new(),
            seen: HashSet::new(),
        }
    }

    fn resolve(&mut self, name: &str) -> Option<u32> {
        if let Some(&r) = self.resolved.get(name) {
            return r;
        }
        let r = lines::canonical(name)
            .filter(|p| p.starts_with(self.root))
            .map(|p| self.places.path(&relative(self.root, &p)));
        self.resolved.insert(name.to_string(), r);
        r
    }

    /// Add the project items of unit `path` not already taken, recorded as `<path>.i`.
    fn add(&mut self, path: &str, read: check::Read) {
        let check::Read {
            mapped,
            parsed,
            graph,
        } = read;
        let files: Vec<Option<u32>> = mapped.files.iter().map(|f| self.resolve(f)).collect();
        // Top-level items whose bytes overlap (declarations sharing a specifier) go together.
        let mut groups: Vec<(Vec<usize>, u32, u32)> = vec![];
        for &root in &graph.roots {
            let span = graph.nodes[root].span;
            match groups.last_mut() {
                Some((roots, _, hi)) if span.lo < *hi => {
                    roots.push(root);
                    *hi = (*hi).max(span.hi);
                }
                _ => groups.push((vec![root], span.lo, span.hi)),
            }
        }
        let mut text: Vec<u8> = vec![];
        let mut spans = vec![];
        let mut lines = vec![];
        let mut keep: Vec<(usize, String)> = vec![];
        for (roots, lo, hi) in groups {
            let Some((_, file, line)) = mapped.lines_in(lo, hi).next() else {
                continue;
            };
            let Some(module) = files[file as usize] else {
                continue;
            };
            let bytes = &graph.source[lo as usize..hi as usize];
            if !self.seen.insert((module, line, bytes.to_vec())) {
                continue;
            }
            let at = text.len() as u32;
            spans.push((lo, hi, at));
            lines.extend(
                mapped
                    .lines_in(lo, hi)
                    .map(|(start, f, l)| (at + (start - lo), files[f as usize], l)),
            );
            text.extend_from_slice(bytes);
            text.push(b'\n');
            let name = self.places.name(module).to_string();
            keep.extend(roots.into_iter().map(|r| (r, name.clone())));
        }
        if keep.is_empty() {
            return;
        }
        let id = FileId(self.sources.len() as u64 + 1);
        let ingested = format!("{}.i", path.strip_suffix(".c").unwrap_or(path));
        let source = SourceFile::new(id, ingested, String::from_utf8_lossy(&text).into_owned());
        self.program.add(
            &graph,
            &parsed.ast,
            &source,
            &Placement::spans(id, spans),
            &keep,
        );
        self.places.add(id, lines);
        self.sources.push(source);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_load_stops_between_files() {
        let dir = std::env::temp_dir().join(format!("argo-cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            std::fs::write(
                dir.join(format!("f{i}.c")),
                format!("int f{i}(void) {{ return {i}; }}\n"),
            )
            .unwrap();
        }
        let opts = Options::default();
        opts.cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            open(&dir, "c", &opts, &|_| {}).err().as_deref(),
            Some(git::CANCELLED)
        );

        // Cancelled while checking: the files after it aren't read and nothing is merged.
        let opts = Options::default();
        let merged = AtomicBool::new(false);
        let r = open(&dir, "c", &opts, &|step| match step {
            Step::Checking { .. } => opts.cancel.store(true, Ordering::Relaxed),
            Step::Merging { .. } => merged.store(true, Ordering::Relaxed),
            _ => {}
        });
        assert_eq!(r.err().as_deref(), Some(git::CANCELLED));
        assert!(!merged.load(Ordering::Relaxed));
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
