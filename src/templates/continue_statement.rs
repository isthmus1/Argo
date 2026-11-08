use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// Continue is recognized without guessing its enclosing loop's graph exits.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContinueTemplate;

impl Template for ContinueTemplate {
    fn id(&self) -> &'static str {
        "continue"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::Continue
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::straight_line(node)
    }
}
