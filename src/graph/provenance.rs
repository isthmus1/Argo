use rucc_diag::Span;
use serde::Serialize;

use super::ControlExitKind;
use crate::CNodeRole;

/// Half-open byte range in the graph's retained original source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceSpan {
    pub lo: u32,
    pub hi: u32,
}

impl From<Span> for SourceSpan {
    fn from(span: Span) -> Self {
        Self {
            lo: span.lo,
            hi: span.hi,
        }
    }
}

impl SourceSpan {
    pub fn source(self, source: &[u8]) -> Option<&[u8]> {
        source.get(self.lo as usize..self.hi as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxPart {
    pub role: CNodeRole,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ControlExit {
    pub kind: ControlExitKind,
    pub span: SourceSpan,
}
