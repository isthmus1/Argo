use std::ops::Range;

use crate::CNode;

/// One AST node's exact C text and where it sits in the parsed source, ready
/// to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CSnippet {
    /// The `.c` (or header) file the node was parsed from.
    pub file: String,
    /// Byte range of the node in the parsed source (`text == source[range]`).
    pub range: Range<usize>,
    /// The node's C, copied byte for byte from the source.
    pub text: String,
}

impl CSnippet {
    /// `node`'s text and range in `source`, which must be the source that was
    /// parsed from `file`. `None` if the range is out of bounds or the bytes
    /// aren't UTF-8.
    pub fn from_node(node: &CNode, source: &[u8], file: &str) -> Option<Self> {
        let range = node.span.lo as usize..node.span.hi as usize;
        let text = std::str::from_utf8(source.get(range.clone())?).ok()?;
        Some(Self {
            file: file.to_string(),
            range,
            text: text.to_string(),
        })
    }
}
