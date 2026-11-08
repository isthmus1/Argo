use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// Expression and initializer syntax; this does not imply purity/evaluation.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExpressionTemplate;

impl Template for ExpressionTemplate {
    fn id(&self) -> &'static str {
        "expression"
    }

    fn matches(&self, node: &CNode) -> bool {
        matches!(
            node.node_type,
            CNodeType::Expression | CNodeType::Initializer
        )
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::straight_line(node)
    }
}
