use super::{GraphError, NodeDraft, SourceSpan};

pub(super) fn span(source: &[u8], span: SourceSpan) -> Result<(), GraphError> {
    if span.source(source).is_none() {
        Err(GraphError::InvalidSpan(span))
    } else {
        Ok(())
    }
}

fn contained(outer: SourceSpan, inner: SourceSpan) -> Result<(), GraphError> {
    if inner.lo < outer.lo || inner.hi > outer.hi {
        Err(GraphError::OutsideRegion { outer, inner })
    } else {
        Ok(())
    }
}

pub(super) fn draft(
    draft: &NodeDraft,
    source: &[u8],
    parent: Option<SourceSpan>,
) -> Result<(), GraphError> {
    span(source, draft.span)?;
    if let Some(parent) = parent {
        contained(parent, draft.span)?;
    }
    for part in &draft.parts {
        span(source, part.span)?;
        contained(draft.span, part.span)?;
    }
    for exit in &draft.exits {
        span(source, exit.span)?;
        contained(draft.span, exit.span)?;
    }
    Ok(())
}
