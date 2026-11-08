use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// While syntax preserves pre-test condition/body order.
#[derive(Debug, Default, Clone, Copy)]
pub struct WhileTemplate;

impl Template for WhileTemplate {
    fn id(&self) -> &'static str {
        "while"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::While
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::loop_region(node)
    }
}
