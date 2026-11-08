//! The translation prompt: the chunk's exact preprocessed C from the database, its original lines
//! and their surroundings from the clone, and the previous attempt's failure.

use std::path::{Component, Path};

use argo_db_client::CChunk;
use iterator::nemo::getLines;

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

/// The prompt for `chunk`. `checkout` is the clone at the ingested commit, if there is one.
/// The rules assume a function: the C front end makes only functions work units. A translatable
/// `Region` (fixtures only, so far) has no C symbol and will need its own outlining rules here.
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
