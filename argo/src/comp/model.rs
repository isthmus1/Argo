use std::ops::Range;
use std::path::PathBuf;

/// A piece of a C file swapped for Rust: the C at `span` in `file` is replaced, and the Rust
/// (exporting `symbol` through `extern "C"`) is linked in to provide what the replacement calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// The original C file the span is in, relative to the project's root.
    pub file: PathBuf,
    /// Byte range in `file` (or, for [`super::run_with_c`], in the C source it is given).
    pub span: Range<usize>,
    /// Rust that exports `symbol` with `#[no_mangle] pub extern "C" fn`.
    pub rust: String,
    /// The C put at `span`: for a piece of a function, the call to `symbol` that replaces it.
    /// `None` for a whole function definition, which becomes its declaration.
    pub c: Option<String>,
    /// The function the Rust must export.
    pub symbol: String,
}
