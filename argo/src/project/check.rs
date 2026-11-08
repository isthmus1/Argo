//! Each `.c` file alone: preprocessed, parsed and built, in parallel. Files that fail here are
//! left out of the combined unit with the reason.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::Step;
use super::cpp::{self, Input};
use super::lines::Mapped;
use crate::{Parser, templates};

/// Why each file can't be read alone, or `None` when it can.
pub fn all(
    cc: &str,
    flags: &[String],
    sources: &[std::path::PathBuf],
    progress: &(dyn Fn(Step) + Sync),
) -> Vec<Option<String>> {
    let total = sources.len();
    let done = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, total.max(1));
    let per = total.div_ceil(threads).max(1);
    let mut verdicts: Vec<Option<String>> = vec![None; total];
    std::thread::scope(|scope| {
        for (ci, chunk) in verdicts.chunks_mut(per).enumerate() {
            let done = &done;
            scope.spawn(move || {
                for (k, verdict) in chunk.iter_mut().enumerate() {
                    *verdict = one(cc, flags, &sources[ci * per + k]).err();
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(Step::Checking { done: n, total });
                }
            });
        }
    });
    verdicts
}

/// Preprocess, parse and build one file alone.
fn one(cc: &str, flags: &[String], path: &Path) -> Result<(), String> {
    let raw = cpp::preprocess(cc, flags, Input::File(path))?;
    let mapped = Mapped::new(&raw);
    let parsed = Parser::default().parse(&mapped.text);
    if let Some(d) = parsed.diagnostics.iter().find(|d| d.severity.is_fatal()) {
        let o = mapped.origin(d.span.lo);
        let file = o.file.rsplit('/').next().unwrap_or(o.file);
        return Err(format!("{} ({file}:{})", d.message, o.line));
    }
    parsed
        .ast
        .build_graph(&parsed.source, templates::all())
        .map(|_| ())
        .map_err(|e| e.to_string())
}
