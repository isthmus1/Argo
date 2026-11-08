use std::ops::Range;

use crate::CNode;

/// One AST node's exact C text and where it sits in the parsed source, ready
/// to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CSnippet {
    /// Byte range of the node in the parsed source (`text == source[range]`).
    pub range: Range<usize>,
    /// The node's C, copied byte for byte from the source.
    pub text: String,
}

impl CSnippet {
    /// `node`'s text and range in `source`, which must be the source that was
    /// parsed. `None` if the range is out of bounds or the bytes aren't UTF-8.
    pub fn from_node(node: &CNode, source: &[u8]) -> Option<Self> {
        let range = node.span.lo as usize..node.span.hi as usize;
        let text = std::str::from_utf8(source.get(range.clone())?).ok()?;
        Some(Self {
            range,
            text: text.to_string(),
        })
    }
}
