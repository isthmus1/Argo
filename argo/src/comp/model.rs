use std::ops::Range;

/// One C function definition to swap for Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    /// Byte range of the C function definition in the C source.
    pub span: Range<usize>,
    /// Rust that exports the same function with
    /// `#[unsafe(no_mangle)] pub extern "C" fn ...`.
    pub rust: String,
}
