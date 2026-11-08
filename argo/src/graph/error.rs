use std::{error::Error, fmt};

use super::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    InvalidSpan(SourceSpan),
    OutsideRegion {
        outer: SourceSpan,
        inner: SourceSpan,
    },
    NoTemplate(SourceSpan),
    EmptyRegion,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRegion => f.write_str("a graph child region cannot be empty"),
            Self::OutsideRegion { outer, inner } => write!(
                f,
                "source span {}..{} escapes region {}..{}",
                inner.lo, inner.hi, outer.lo, outer.hi
            ),
            Self::InvalidSpan(span) => write!(f, "invalid source span {}..{}", span.lo, span.hi),
            Self::NoTemplate(span) => {
                write!(f, "no graph template accepted {}..{}", span.lo, span.hi)
            }
        }
    }
}

impl Error for GraphError {}
