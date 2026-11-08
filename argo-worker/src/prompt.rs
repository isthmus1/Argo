//! The translation prompt: the chunk's exact preprocessed C from the database, its original lines
//! and their surroundings from the clone, and the previous attempt's failure.

use std::path::{Component, Path};

use argo_db_client::CChunk;
use iterator::nemo::getLines;

use crate::job::{Job, Piece};
use crate::log::tail;

/// Lines of the original file shown on each side of the chunk.
const CONTEXT_LINES: u32 = 20;
/// Feedback beyond this is cut from the front; the end of a compiler log matters most.
const FEEDBACK_BYTES: usize = 16 << 10;

/// The rules, after iterator's prompt (code only, exact C semantics), plus the ABI contract the
/// candidate is linked back into the C program under (§5.9).
const RULES: &str = "\
Rewrite the TARGET C code in Rust so it can be linked into the C program in its place.
- Output only Rust code. Anything else must be a Rust comment. No markdown fences.
- Export it as `#[no_mangle] pub extern \"C\" fn` with the exact C symbol name and an \
ABI-identical signature: `core::ffi` scalar types, raw pointers, `#[repr(C)]` types. A C \
name that is a Rust keyword is spelled as a raw identifier (`fn r#type`).
- Declare every C function or global it uses in an `extern \"C\"` block; don't redefine them.
- Preserve exact C semantics: integers are truthy when nonzero (`while (x)` is \
`while x != 0`), keep signedness, overflow and pointer behavior. Every reassigned local is \
`let mut`.
- Never let a panic unwind across the FFI boundary. Use `unsafe` only where C pointers need it.
- The Rust must compile.
";

/// The prompt for a job: a whole function's, or [`piece`]'s for a piece of one.
pub fn for_job(job: &Job<'_>, feedback: Option<&str>) -> String {
    match &job.piece {
        Some(p) => piece(job.chunk, p, job.checkout, feedback),
        None => build(job.chunk, job.checkout, feedback),
    }
}

/// The rules for a piece of a function: it becomes its own exported Rust function, and the C
/// lines it was are replaced by a call to it.
const PIECE_RULES: &str = "\
Move the TARGET lines of the C function below out into their own Rust function, so they can be \
replaced in the C program by a call to it.
- Answer with exactly two fenced blocks: first ```rust, then ```c. Nothing else.
- The ```rust block: `#[no_mangle] pub extern \"C\" fn SYMBOL(...)` doing exactly what the \
TARGET lines do. Pass each local variable or parameter the lines read as an argument, and each \
one they assign as a pointer. Use `core::ffi` scalar types, raw pointers and `#[repr(C)]` \
types for C structs (or `*mut core::ffi::c_void` when only passed through). Declare every C \
function or global the lines use in an `extern \"C\"` block. Never let a panic unwind across \
the boundary.
- The ```c block: the C that replaces the TARGET lines, word for word in their place: a \
prototype `T SYMBOL(...);` for the Rust function, then the call. Keep any declaration the rest \
of the function still uses (`int x = SYMBOL(...);`), and keep any `return`, `break`, \
`continue` or `goto` the lines had, driven by what the call returns.
- Both must compile: the C in its place inside the original file, the Rust on its own.
";

/// The prompt for a piece of a function (a `Region` row).
fn piece(chunk: &CChunk, p: &Piece<'_>, checkout: Option<&Path>, feedback: Option<&str>) -> String {
    let mut out = format!("{PIECE_RULES}\nSYMBOL: `{}`\n", p.symbol);
    out.push_str(&format!(
        "\nTHE WHOLE FUNCTION (preprocessed C, for reference; do NOT output it):\n{}\n",
        p.function.source
    ));
    let lines = checkout.and_then(|root| lines_of(chunk, root));
    out.push_str(&format!(
        "\nTARGET (lines {}-{} of {}, exactly as the file has them; these are replaced):\n{}\n",
        chunk.line_start,
        chunk.line_end,
        chunk.path,
        lines.as_deref().unwrap_or(&chunk.source)
    ));
    if let Some(log) = feedback {
        out.push_str(&format!(
            "\nYOUR PREVIOUS ATTEMPT WAS REJECTED. Fix this:\n{}\n",
            tail(log, FEEDBACK_BYTES)
        ));
    }
    out
}

/// The row's own original lines, as the file in the clone has them.
fn lines_of(chunk: &CChunk, root: &Path) -> Option<String> {
    let path = Path::new(&chunk.path);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if chunk.path.is_empty() || !plain || chunk.line_start == 0 {
        return None;
    }
    let real = root.join(path).canonicalize().ok()?;
    if !real.starts_with(root.canonicalize().ok()?) {
        return None;
    }
    let file = std::fs::read_to_string(real).ok()?;
    Some(getLines(&file, chunk.line_start, chunk.line_end))
}

/// The prompt for a whole function's row. `checkout` is the clone at the ingested commit, if
/// there is one.
pub fn build(chunk: &CChunk, checkout: Option<&Path>, feedback: Option<&str>) -> String {
    let mut out = format!("{RULES}\nSymbol: `{}`\n", chunk.name);
    if let Some(text) = checkout.and_then(|root| original(chunk, root)) {
        out.push_str(&text);
    }
    out.push_str(&format!(
        "TARGET (preprocessed C, the exact code to rewrite):\n{}\n",
        chunk.source
    ));
    if let Some(log) = feedback {
        out.push_str(&format!(
            "\nYOUR PREVIOUS ATTEMPT WAS REJECTED. Fix this:\n{}\n",
            tail(log, FEEDBACK_BYTES)
        ));
    }
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
        "CONTEXT ({path} lines {from}-{to}, before preprocessing; reference only, do NOT \
         output; the target is lines {start}-{end}):\n{}\n",
        getLines(&file, from, to),
        path = chunk.path,
    ))
}

#[cfg(test)]
mod tests;
