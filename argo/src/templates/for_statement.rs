use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// The for clauses and body stay together; this does not outline the loop.
#[derive(Debug, Default, Clone, Copy)]
pub struct ForTemplate;

impl Template for ForTemplate {
    fn id(&self) -> &'static str {
        "for"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::For
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::loop_region(node)
    }
}
