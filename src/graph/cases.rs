use super::{Expansion, GraphError, GraphNodeKind, SourceSpan, SyntaxPart, regions};
use crate::{CNode, CNodeRole as Role, CNodeType as Kind};

pub(super) fn expand<'a>(
    labels: &[&'a CNode],
    statements: &[&'a CNode],
) -> Result<Expansion<'a>, GraphError> {
    let first = *labels.first().ok_or(GraphError::EmptyRegion)?;
    let mut expansion = Expansion::leaf(GraphNodeKind::CaseGroup, first);
    let draft = expansion.node.as_mut().unwrap();
    draft.name = None;
    draft.role = None;
    draft.span.hi = labels
        .iter()
        .chain(statements)
        .map(|node| node.span.hi)
        .max()
        .unwrap();
    for label in labels {
        if label.node_type == Kind::Default {
            let end = label
                .children
                .iter()
                .find(|node| node.role == Some(Role::Body))
                .map_or(label.span.hi, |node| node.span.lo);
            draft.parts.push(SyntaxPart {
                role: Role::DefaultLabel,
                span: SourceSpan {
                    lo: label.span.lo,
                    hi: end,
                },
            });
        } else {
            for value in &label.children {
                if let Some(role @ (Role::CaseValue | Role::CaseEnd)) = value.role {
                    draft.parts.push(SyntaxPart {
                        role,
                        span: value.span.into(),
                    });
                }
            }
        }
    }
    expansion.children = regions::partition(statements.iter().copied());
    Ok(expansion)
}

/// Only unconditional direct exits prove no fallthrough. Uncertain cases stay joined.
pub(super) fn terminates(node: &CNode) -> bool {
    match node.node_type {
        Kind::Break | Kind::Return | Kind::Continue => true,
        Kind::Block => node.children.last().is_some_and(terminates),
        _ => false,
    }
}
