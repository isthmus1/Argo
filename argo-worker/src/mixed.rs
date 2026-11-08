//! `argo export --mixed`: the C program with every accepted piece's C call spliced in, beside
//! the exported Rust crate, so the project's own build links the two into one program.
//!
//! The tree is a copy of the clone (at the ingested commit). Each C file with accepted rows is
//! replaced by its preprocessed unit (macros expanded, as the worker compiled it) with each
//! piece's text replaced by its `c_call`, and each translated whole function's definition
//! replaced by its prototype (its Rust is then the definition). Spliced last to first, so the
//! byte offsets found in the original text stay right.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;

use argo_c::project::make;
use argo_db_client::{CChunk, ChunkKind, RustChunk, RustStatus};

use crate::program::{Build, Toolchain};

/// What was spliced, for the report.
pub struct Mixed {
    pub files: usize,
    pub pieces: usize,
    pub functions: usize,
    /// Accepted rows that couldn't be placed, with why.
    pub skipped: Vec<(u64, String)>,
}

/// Write the mixed tree for `checkout` (the clone, at the ingested commit) to `out`.
pub fn write(
    chunks: &[CChunk],
    rusts: &[RustChunk],
    units: &[String],
    checkout: &Path,
    out: &Path,
) -> Result<Mixed, String> {
    let tools = Toolchain { cc: std::env::var("CC").unwrap_or_else(|_| "cc".into()), libs: vec![] };
    let root = checkout.canonicalize().map_err(|e| format!("{}: {e}", checkout.display()))?;
    let build = Build::new(&root, units, &tools)?;
    let makefile = make::read(&root);
    let by_id: HashMap<u64, &CChunk> = chunks.iter().map(|c| (c.id, c)).collect();
    let done: HashMap<u64, &RustChunk> =
        rusts.iter().filter(|r| r.status == RustStatus::Done).map(|r| (r.chunk, r)).collect();
    let function_of = |c: &CChunk| {
        let mut at = c.parent;
        while let Some(id) = at {
            let row = by_id.get(&id)?;
            if row.kind == ChunkKind::Function {
                return Some(*row);
            }
            at = row.parent;
        }
        None
    };

    // Per unit: its text, and the (lo, hi, replacement) edits in it.
    let mut edits: BTreeMap<String, (String, Vec<(usize, usize, String)>)> = BTreeMap::new();
    let mut m = Mixed { files: 0, pieces: 0, functions: 0, skipped: vec![] };
    for c in chunks.iter().filter(|c| done.contains_key(&c.id)) {
        let placed = match c.kind {
            ChunkKind::Function => build.locate(c, &root, &makefile, &tools).and_then(|(path, text, at)| {
                // The definition goes; its prototype stays, for the C that calls it.
                let head = c.source.find('{').ok_or("no body")?;
                let proto = format!("{};", c.source[..head].trim_end());
                Ok((path, text, at, at + c.source.len(), proto))
            }),
            _ => {
                let Some(function) = function_of(c) else {
                    m.skipped.push((c.id, "no enclosing function".into()));
                    continue;
                };
                if done.contains_key(&function.id) {
                    continue; // The whole function is Rust: its pieces go with it.
                }
                let c_call = c.c_call.clone();
                if c_call.trim().is_empty() {
                    m.skipped.push((c.id, "no C call stored".into()));
                    continue;
                }
                build.locate(function, &root, &makefile, &tools).and_then(|(path, text, at)| {
                    let within = c
                        .byte_lo
                        .checked_sub(function.byte_lo)
                        .map(|r| r as usize)
                        .filter(|&r| function.source.get(r..r + c.source.len()) == Some(c.source.as_str()))
                        .or_else(|| {
                            let found: Vec<usize> =
                                function.source.match_indices(&c.source).map(|(i, _)| i).collect();
                            (found.len() == 1).then(|| found[0])
                        })
                        .ok_or("the piece's text isn't found in its function")?;
                    let lo = at + within;
                    Ok((path, text, lo, lo + c.source.len(), c_call))
                })
            }
        };
        match placed {
            Ok((path, text, lo, hi, with)) => {
                let entry = edits.entry(path).or_insert_with(|| (text, vec![]));
                entry.1.push((lo, hi, with));
                match c.kind {
                    ChunkKind::Function => m.functions += 1,
                    _ => m.pieces += 1,
                }
            }
            Err(why) => m.skipped.push((c.id, why)),
        }
    }

    // A copy of the clone, then each edited unit over its file.
    if !out.exists() {
        let status = Command::new("cp").arg("-a").arg(&root).arg(out).status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("copying {} to {} failed", root.display(), out.display()));
        }
    }
    for (path, (text, mut cuts)) in edits {
        cuts.sort_by_key(|&(lo, _, _)| std::cmp::Reverse(lo));
        let mut text = text;
        let mut floor = usize::MAX;
        for (lo, hi, with) in cuts {
            if hi > floor {
                continue; // Overlaps a later cut already made: keep the outer one.
            }
            text.replace_range(lo..hi, &with);
            floor = lo;
        }
        std::fs::write(out.join(&path), text).map_err(|e| format!("{path}: {e}"))?;
        m.files += 1;
    }
    Ok(m)
}
