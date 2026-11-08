//! Each `.c` file alone, as its own translation unit: preprocessed, parsed and built, in
//! parallel. Files that fail are left out with the reason; the rest are handed on to be merged.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::Step;
use super::cpp::{self, Input};
use super::lines::Mapped;
use super::{dialect, files};
use crate::graph::Graph;
use crate::{ParseResult, Parser, templates};

/// At most this many optional Makefile features are turned off for one file.
const MAX_DROPS: usize = 3;

/// What checking one file found.
pub struct Verdict {
    /// Flags for this file only, after the shared ones: its Makefile rule's, plus `-UNAME` for
    /// each optional feature it had to be read without.
    pub flags: Vec<String>,
    /// Why it can't be read, or `None` when it can.
    pub skipped: Option<String>,
}

/// A file that was read: its preprocessed text with the line map, the parse and the graph.
pub struct Read {
    pub mapped: Mapped,
    pub parsed: ParseResult,
    pub graph: Graph,
}

pub struct Setup<'a> {
    pub cc: &'a str,
    pub root: &'a Path,
    /// Flags shared by every file.
    pub flags: &'a [String],
    /// `-D` flags among `flags` a file may go without (Makefile feature switches).
    pub optional: &'a [String],
    /// Once set, the files not yet checked are skipped as cancelled.
    pub cancel: &'a AtomicBool,
}

/// Check every file of `sources`, each starting from its own extra flags, and hand each one
/// read to `read` in the order of `sources`, on the calling thread. Files are read a batch at a
/// time so only a batch's syntax trees are held at once. Files reached after `setup.cancel` is set
/// are not read.
pub fn all(
    setup: &Setup<'_>,
    sources: &[(PathBuf, Vec<String>)],
    progress: &(dyn Fn(Step) + Sync),
    read: &mut dyn FnMut(usize, Read),
) -> Vec<Verdict> {
    let total = sources.len();
    let done = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, total.max(1));
    let mut verdicts = Vec::with_capacity(total);
    for start in (0..total).step_by(threads * 2) {
        let batch = &sources[start..(start + threads * 2).min(total)];
        let next = AtomicUsize::new(0);
        let results: Vec<Mutex<Option<(Verdict, Option<Read>)>>> =
            batch.iter().map(|_| Mutex::new(None)).collect();
        std::thread::scope(|scope| {
            for _ in 0..threads.min(batch.len()) {
                scope.spawn(|| {
                    loop {
                        let k = next.fetch_add(1, Ordering::Relaxed);
                        let Some((path, extra)) = batch.get(k) else {
                            break;
                        };
                        let result = if setup.cancel.load(Ordering::Relaxed) {
                            let skipped = Some(super::git::CANCELLED.into());
                            (
                                Verdict {
                                    flags: extra.clone(),
                                    skipped,
                                },
                                None,
                            )
                        } else {
                            one(setup, path, extra.clone())
                        };
                        *results[k].lock().expect("no reader panicked") = Some(result);
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(Step::Checking { done: n, total });
                    }
                });
            }
        });
        for (k, result) in results.into_iter().enumerate() {
            let (verdict, unit) = result
                .into_inner()
                .expect("no reader panicked")
                .expect("every file of the batch was checked");
            if let Some(unit) = unit {
                read(start + k, unit);
            }
            verdicts.push(verdict);
        }
    }
    verdicts
}

/// Preprocess, parse and build one file alone. When a header is missing, try turning off the
/// optional features one at a time: `ifdef CONFIG_FS_NET` in the Makefile adds `-DCONFIG_FS_NET`,
/// which makes `temu.c` include `<openssl/aes.h>`; without OpenSSL installed the file is still
/// read, as built without that feature.
fn one(setup: &Setup<'_>, path: &Path, mut extra: Vec<String>) -> (Verdict, Option<Read>) {
    let run = |extra: &[String]| {
        let flags: Vec<String> = setup.flags.iter().chain(extra).cloned().collect();
        cpp::preprocess(setup.cc, &flags, Input::File(path))
    };
    let mut raw = run(&extra);
    let mut drops = 0;
    while let Err(e) = &raw {
        let Some(header) = e.missing_header().map(str::to_string) else {
            break;
        };
        if drops == MAX_DROPS {
            break;
        }
        let found = setup.optional.iter().find_map(|d| {
            let name = d[2..].split('=').next().unwrap_or_default();
            let undef = format!("-U{name}");
            if extra.contains(&undef) {
                return None;
            }
            let mut trial = extra.clone();
            trial.push(undef);
            let r = run(&trial);
            // Progress is reading the file, or only missing another header (which a further
            // switch may cover); turning off a define the file requires (`#error`) is not.
            let progress = match &r {
                Ok(_) => true,
                Err(e) => e.missing_header().is_some_and(|h| h != header),
            };
            progress.then_some((trial, r))
        });
        let Some((trial, r)) = found else { break };
        extra = trial;
        raw = r;
        drops += 1;
    }
    let (skipped, unit) = match raw.map(|raw| parse(&raw, setup.root)) {
        Ok(Ok(unit)) => (None, Some(unit)),
        Ok(Err(why)) => (Some(why), None),
        Err(e) => (Some(e.describe(|f| files::short(f, setup.root))), None),
    };
    let verdict = Verdict {
        flags: extra,
        skipped,
    };
    (verdict, unit)
}

fn parse(raw: &[u8], root: &Path) -> Result<Read, String> {
    let mut mapped = Mapped::new(raw);
    dialect::neutralize(&mut mapped.text);
    let parsed = Parser::default().parse(&mapped.text);
    if let Some(d) = parsed.diagnostics.iter().find(|d| d.severity.is_fatal()) {
        let o = mapped.origin(d.span.lo);
        return Err(format!(
            "{} ({}:{})",
            d.message,
            files::short(o.file, root),
            o.line
        ));
    }
    let graph = parsed
        .ast
        .build_graph(&parsed.source, templates::all())
        .map_err(|e| e.to_string())?;
    Ok(Read {
        mapped,
        parsed,
        graph,
    })
}
