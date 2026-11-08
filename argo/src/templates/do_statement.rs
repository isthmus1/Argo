use super::Template;
use crate::graph::{Expansion, Unsupported};
use crate::{CNode, CNodeType};

/// A do/while loop retains body-before-condition syntax.
#[derive(Debug, Default, Clone, Copy)]
pub struct DoWhileTemplate;

impl Template for DoWhileTemplate {
    fn id(&self) -> &'static str {
        "do"
    }

    fn matches(&self, node: &CNode) -> bool {
        node.node_type == CNodeType::DoWhile
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        crate::graph::constructs::loop_region(node)
    }
}
