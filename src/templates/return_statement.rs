use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// Return is a syntax node here, not a standalone executable graph work unit.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReturnTemplate;

impl Template for ReturnTemplate {
    fn id(&self) -> &'static str {
        "return"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Return
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::straight_line(node)
    }
}
