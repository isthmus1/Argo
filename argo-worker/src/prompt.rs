//! The conversation's start: a system message (the job, the tools, the workflow and the ABI
//! contract the candidate is linked under, §5.9), and the task: the chunk's exact preprocessed
//! C from the database, its original lines and their surroundings from the clone, and which
//! checks a submission goes through.

use std::fmt::Write as _;
use std::path::{Component, Path};

use argo_db_client::CChunk;

use crate::agent::{Budget, Task};
use crate::model::Message;

/// Lines of the original file shown on each side of the chunk.
const CONTEXT_LINES: u32 = 20;

/// The rules: exact C semantics, plus the ABI contract.
const RULES: &str = "\
- Export it as `#[no_mangle] pub extern \"C\" fn` with the exact C symbol name and an \
ABI-identical signature: `core::ffi` scalar types (`c_int`, `c_long`, `c_char`, ...), raw \
pointers, `#[repr(C)]` types. A C name that is a Rust keyword is spelled as a raw identifier \
(`fn r#type`).
- Declare every C function or global it uses in an `extern \"C\"` block with the exact C types; \
don't redefine them. A `static` C function has no linker symbol: port its logic into the file \
as a private Rust fn instead.
- Mirror a struct you access through a pointer with `#[repr(C)]` and exactly the C fields, in \
order, with the same types.
- Preserve exact C semantics: integers are truthy when nonzero (`while (x)` is \
`while x != 0`), keep signedness, integer widths, wrapping overflow and pointer behavior. \
Every reassigned local is `let mut`.
- Never let a panic unwind across the FFI boundary: no `unwrap`, `expect` or slice indexing on \
C data. Use `unsafe` only where C pointers need it.
- Only `core` and `std`; no other crates.";

fn system(budget: &Budget) -> String {
    format!(
        "You are a porting agent. You rewrite one C function, the TARGET, in Rust; the Rust is \
linked into the C program in place of the C definition, and the rest of the program stays C.

Tools:
- read(path, offset?, limit?): read a file with line numbers. Paths are relative to the \
repository root; files in the session directory are read by their absolute path.
- grep(pattern, path?, glob?, ignore_case?): regex search over file contents.
- glob(pattern, path?): find files by path pattern.
- submit_rewrite(rust_source): submit the complete Rust file. It is compiled, checked against \
the C program's symbols, linked into the C program, and its exported signature is compared \
with the C function's ABI. If every check passes it is accepted and you are done. If not, you \
are told what failed, why, and where the full logs are.
You cannot write files or run commands: submit_rewrite is the only way to build code.

How to work:
1. Read the TARGET. It is preprocessed: macros are expanded and comments are gone. The CONTEXT \
shows the original source around it.
2. Before writing, look up what you need: the definitions of the struct types it uses, the \
signatures of the functions it calls, the globals it touches. Search the repository, or the \
TARGET's preprocessed translation unit in the session directory, which holds every \
declaration the TARGET can see.
3. Submit the whole file. When it is rejected, fix every cause listed and submit the whole \
corrected file again. After {} rejections the chunk fails, so check your code before \
submitting.

Rules for the Rust:
{RULES}
- `rust_source` is the raw file text: Rust only, no markdown fences, no prose outside // \
comments.",
        budget.max_attempts
    )
}

/// The opening messages for `task`.
pub fn conversation(task: &Task<'_>, budget: &Budget) -> Vec<Message> {
    vec![Message::System(system(budget)), Message::User(brief(task))]
}

fn brief(task: &Task<'_>) -> String {
    let chunk = task.chunk;
    let mut out = format!("Symbol: `{}`\n", chunk.name);
    let program = task.program.as_deref();
    if let Some(p) = program.filter(|p| !p.declaration.is_empty()) {
        let _ = writeln!(out, "C declaration: `{}`", p.declaration);
    }
    if !chunk.path.is_empty() && chunk.line_start > 0 {
        let _ = writeln!(
            out,
            "Original location: {} lines {}-{}",
            chunk.path, chunk.line_start, chunk.line_end
        );
    }
    match task.checkout {
        Some(_) => out.push_str("Repository: available (paths are relative to its root)\n"),
        None => out.push_str("Repository: not available for this chunk\n"),
    }
    let _ = writeln!(out, "Session directory: {}", task.session.display());
    if let Some(unit) = program.and_then(|p| p.preprocessed.as_ref()) {
        let _ = writeln!(
            out,
            "The TARGET's translation unit, preprocessed (every declaration it can see): {}",
            unit.display()
        );
    }
    match program {
        Some(p) if !p.objects.is_empty() && p.signature.is_some() => {
            out.push_str("Checks on submit: rustc, symbols, link into the C program, ABI.\n");
        }
        _ => out.push_str("Checks on submit: rustc only (no linkable C program).\n"),
    }
    for note in program.map(|p| p.notes.as_slice()).unwrap_or_default() {
        let _ = writeln!(out, "Note: {note}");
    }
    out.push('\n');
    if let Some(text) = task.checkout.and_then(|root| original(chunk, root)) {
        out.push_str(&text);
    }
    let _ = writeln!(
        out,
        "TARGET (preprocessed C, the exact code to rewrite):\n{}",
        chunk.source
    );
    out
}

/// The chunk's original lines with their surroundings, for reference only.
fn original(chunk: &CChunk, root: &Path) -> Option<String> {
    let (start, end) = (chunk.line_start, chunk.line_end);
    // Only plain relative paths: a row must not point the worker outside its clone.
    let path = Path::new(&chunk.path);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if chunk.path.is_empty() || !plain || start == 0 {
        return None;
    }
    // Nor may a symlink in the clone: whatever is read goes to the model.
    let real = root.join(path).canonicalize().ok()?;
    if !real.starts_with(root.canonicalize().ok()?) {
        return None;
    }
    let file = std::fs::read_to_string(real).ok()?;
    let from = start.saturating_sub(CONTEXT_LINES).max(1);
    let to = end
        .saturating_add(CONTEXT_LINES)
        .min(file.lines().count() as u32);
    Some(format!(
        "CONTEXT ({path} lines {from}-{to}, before preprocessing; reference only; the target \
         is lines {start}-{end}):\n{}\n\n",
        lines(&file, from, to),
        path = chunk.path,
    ))
}

/// 1-based inclusive lines `from..=to` of `text`.
fn lines(text: &str, from: u32, to: u32) -> String {
    let from = from.max(1);
    let skip = (from - 1) as usize;
    let take = (to + 1).saturating_sub(from) as usize;
    text.lines()
        .skip(skip)
        .take(take)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
