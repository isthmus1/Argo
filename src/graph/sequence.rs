use super::{ControlExit, ControlExitKind, Expansion, GraphError, GraphNodeKind, SourceSpan};
use crate::{CNode, CNodeType as Kind};

pub(super) fn expand<'a>(nodes: &[&'a CNode]) -> Result<Expansion<'a>, GraphError> {
    let first = *nodes.first().ok_or(GraphError::EmptyRegion)?;
    let last = nodes.last().unwrap();
    let span = SourceSpan {
        lo: first.span.lo,
        hi: last.span.hi,
    };
    let unsupported = nodes.iter().any(|node| {
        node.walk().any(|child| {
            matches!(
                child.node_type,
                Kind::Opaque | Kind::Goto | Kind::Label | Kind::Block
            )
        })
    });
    let mut expansion = if unsupported {
        Expansion::opaque(first, "unsupported syntax in a straight-line region")
    } else {
        Expansion::leaf(GraphNodeKind::Sequence, first)
    };
    let draft = expansion.node.as_mut().unwrap();
    draft.span = span;
    draft.name = None;
    if !unsupported {
        draft.exits = nodes
            .iter()
            .filter_map(|node| {
                let kind = match node.node_type {
                    Kind::Return => ControlExitKind::Return,
                    Kind::Break => ControlExitKind::Break,
                    Kind::Continue => ControlExitKind::Continue,
                    _ => return None,
                };
                Some(ControlExit {
                    kind,
                    span: node.span.into(),
                })
            })
            .collect();
    }
    Ok(expansion)
}
